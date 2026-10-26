//! the index of a whole repo, saved in '.gargoyle/index/'
//! so later runs only parse the files that changed

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use gargoyle_git::{GitError, Repo, TreeFile};
use serde::{Deserialize, Serialize};

use crate::{Definition, Grammar, Reference, Symbols, grammar_of, parse};

/// where the index is saved, relative to the repo root
pub const INDEX_DIR: &str = ".gargoyle/index";

const CACHE_FILE: &str = "symbols.json";

// bump when parsing changes what it finds, an older cache is then parsed again
const FORMAT: u32 = 1;

/// whats saved, each file's symbols by an id for its content,
/// so a file that moves or comes back isn't parsed again
#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    format: u32,
    version: String,
    symbols: HashMap<String, Symbols>,
}

impl Cache {
    // none when there is no cache, it's broken, or another version wrote it
    fn load(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        let cache: Self = serde_json::from_slice(&bytes).ok()?;
        (cache.format == FORMAT && cache.version == env!("CARGO_PKG_VERSION")).then_some(cache)
    }
}

/// how an update went, so the cli can say why the first one is slower
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Update {
    /// files in the index
    pub files: usize,
    /// files parsed this time, the rest came from the cache
    pub parsed: usize,
    /// there was no usable cache, so every file was parsed
    pub first: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("could not save the index to {}: {source}", path.display())]
    Save { path: PathBuf, source: io::Error },
}

/// what every indexed file in the repo defines and uses
#[derive(Debug, Default)]
pub struct Index {
    /// each file and the id of its content, sorted by path
    files: Vec<(String, String)>,
    symbols: HashMap<String, Symbols>,
    /// each name to where it's defined and used, as file, item positions
    defined: HashMap<String, Vec<(usize, usize)>>,
    used: HashMap<String, Vec<(usize, usize)>>,
}

// a file to parse, source is set when it was already read to get its id
#[derive(Debug)]
struct Job {
    path: String,
    key: String,
    grammar: &'static Grammar,
    source: Option<String>,
}

impl Index {
    /// brings the saved index up to date with the working tree and loads it
    pub fn build(repo: &Repo) -> Result<(Self, Update), IndexError> {
        let files = repo.tree_files()?;
        Self::update(repo.root(), &files)
    }

    /// same as 'build', for a given list of files under root
    pub fn update(root: &Path, files: &[TreeFile]) -> Result<(Self, Update), IndexError> {
        let dir = root.join(INDEX_DIR);
        let old = Cache::load(&dir.join(CACHE_FILE));
        let first = old.is_none();
        let mut old = old.map(|cache| cache.symbols).unwrap_or_default();

        let mut paths = Vec::new();
        let mut jobs = Vec::new();
        let mut queued = HashSet::new();

        for file in files {
            let Some(grammar) = grammar_of(&file.path) else {
                continue;
            };

            let (key, source) = if let Some(blob) = &file.blob {
                (format!("git:{blob}"), None)
            } else {
                // git doesn't have this content, so its id comes from the file itself
                let Some(source) = read(root, &file.path) else {
                    continue;
                };
                let hash = blake3::hash(source.as_bytes()).to_hex();
                (format!("b3:{hash}"), Some(source))
            };

            if !old.contains_key(&key) && queued.insert(key.clone()) {
                jobs.push(Job {
                    path: file.path.clone(),
                    key: key.clone(),
                    grammar,
                    source,
                });
            }

            paths.push((file.path.clone(), key));
        }

        let parsed = jobs.len();
        let mut fresh = parse_all(root, &jobs);

        // only what the tree still has is kept, so the cache never grows stale
        let mut symbols = HashMap::with_capacity(paths.len());

        for (_, key) in &paths {
            if symbols.contains_key(key) {
                continue;
            }

            if let Some(found) = fresh.remove(key).or_else(|| old.remove(key)) {
                symbols.insert(key.clone(), found);
            }
        }

        // file that couldnt be read has nothing to show
        paths.retain(|(_, key)| symbols.contains_key(key));
        paths.sort();

        let cache = Cache {
            format: FORMAT,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            symbols,
        };

        if first || parsed > 0 || !old.is_empty() {
            save(&dir, &cache)?;
        }

        let update = Update {
            files: paths.len(),
            parsed,
            first,
        };

        Ok((Self::new(paths, cache.symbols), update))
    }

    fn new(files: Vec<(String, String)>, symbols: HashMap<String, Symbols>) -> Self {
        let mut defined: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        let mut used: HashMap<String, Vec<(usize, usize)>> = HashMap::new();

        for (f, (_, key)) in files.iter().enumerate() {
            let Some(found) = symbols.get(key) else {
                continue;
            };

            for (i, definition) in found.definitions.iter().enumerate() {
                defined
                    .entry(definition.name.clone())
                    .or_default()
                    .push((f, i));
            }

            for (i, reference) in found.references.iter().enumerate() {
                used.entry(reference.name.clone()).or_default().push((f, i));
            }
        }

        Self {
            files,
            symbols,
            defined,
            used,
        }
    }

    /// what one file defines and uses, none when it isnt indexed
    pub fn file(&self, path: &str) -> Option<&Symbols> {
        let i = self
            .files
            .binary_search_by(|(p, _)| p.as_str().cmp(path))
            .ok()?;

        self.symbols.get(&self.files[i].1)
    }

    /// everywhere a name is define, in path order
    pub fn definitions<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = (&'a str, &'a Definition)> + use<'a> {
        self.found(self.defined.get(name), |symbols, i| {
            symbols.definitions.get(i)
        })
    }

    /// everywhere a name is use in path order
    pub fn references<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = (&'a str, &'a Reference)> + use<'a> {
        self.found(self.used.get(name), |symbols, i| symbols.references.get(i))
    }

    fn found<'a, T: 'a>(
        &'a self,
        positions: Option<&'a Vec<(usize, usize)>>,
        item: fn(&'a Symbols, usize) -> Option<&'a T>,
    ) -> impl Iterator<Item = (&'a str, &'a T)> + use<'a, T> {
        positions.into_iter().flatten().filter_map(move |&(f, i)| {
            let (path, key) = self.files.get(f)?;
            Some((path.as_str(), item(self.symbols.get(key)?, i)?))
        })
    }
}

// parses on every core, each worker taks the next file until none are left
fn parse_all(root: &Path, jobs: &[Job]) -> HashMap<String, Symbols> {
    let next = AtomicUsize::new(0);
    let workers = thread::available_parallelism()
        .map_or(1, NonZeroUsize::get)
        .min(jobs.len());

    thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    while let Some(job) = jobs.get(next.fetch_add(1, Ordering::Relaxed)) {
                        let Some(source) = job.source.clone().or_else(|| read(root, &job.path))
                        else {
                            continue;
                        };

                        done.push((job.key.clone(), parse(job.grammar, &source)));
                    }
                    done
                })
            })
            .collect();

        // worker that panicked only loses its own files
        handles
            .into_iter()
            .filter_map(|handle| handle.join().ok())
            .flatten()
            .collect()
    })
}

// none when the file is gone or isnt text
fn read(root: &Path, path: &str) -> Option<String> {
    String::from_utf8(fs::read(root.join(path)).ok()?).ok()
}

fn save(dir: &Path, cache: &Cache) -> Result<(), IndexError> {
    fs::create_dir_all(dir).map_err(failed(dir))?;

    // index is a cache, git should never see it
    let ignore = dir.join(".gitignore");
    if !ignore.exists() {
        fs::write(&ignore, "*\n").map_err(failed(&ignore))?;
    }

    let path = dir.join(CACHE_FILE);
    let json = serde_json::to_vec(cache)
        .map_err(io::Error::from)
        .map_err(failed(&path))?;

    //written next to it and move in place, so a stopped run never leaves half cache
    let tmp = dir.join(format!("{CACHE_FILE}.tmp"));
    fs::write(&tmp, json).map_err(failed(&tmp))?;
    fs::rename(&tmp, &path).map_err(failed(&path))
}

fn failed(path: &Path) -> impl FnOnce(io::Error) -> IndexError + '_ {
    move |source| IndexError::Save {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree {
        dir: tempfile::TempDir,
        files: Vec<TreeFile>,
    }

    impl Tree {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
                files: Vec::new(),
            }
        }

        // `blob` stands in for git's id, none for a file git doesn't have yet
        fn write(&mut self, path: &str, text: &str, blob: Option<&str>) {
            fs::write(self.dir.path().join(path), text).unwrap();
            self.files.retain(|f| f.path != path);
            self.files.push(TreeFile {
                path: path.to_owned(),
                blob: blob.map(str::to_owned),
            });
        }

        fn update(&self) -> (Index, Update) {
            Index::update(self.dir.path(), &self.files).unwrap()
        }
    }

    #[test]
    fn later_runs_only_parse_what_changed() {
        let mut tree = Tree::new();
        tree.write("a.rs", "fn a() {}\n", Some("1"));
        tree.write("b.rs", "fn b() {}\n", None);
        tree.write("README.md", "# hi\n", None);

        let (index, update) = tree.update();
        assert_eq!(
            update,
            Update {
                files: 2,
                parsed: 2,
                first: true
            }
        );
        assert!(index.file("README.md").is_none(), "no grammar, not indexed");

        assert_eq!(tree.update().1.parsed, 0, "nothing changed");

        tree.write("b.rs", "fn b2() {}\n", None);
        let (index, update) = tree.update();
        assert_eq!((update.parsed, update.first), (1, false));
        assert_eq!(index.definitions("b2").count(), 1);
        assert_eq!(index.definitions("b").count(), 0);

        // same git id, same content, so the cache is trusted
        tree.write("a.rs", "fn other() {}\n", Some("1"));
        assert_eq!(tree.update().1.parsed, 0);
    }

    #[test]
    fn finds_where_a_name_is_defined_and_used() {
        let mut tree = Tree::new();
        tree.write("cart.rs", "pub fn total() -> u64 {\n    0\n}\n", None);
        tree.write("shop.rs", "fn pay() {\n    total();\n}\n", None);
        let (index, _) = tree.update();

        let defined: Vec<_> = index
            .definitions("total")
            .map(|(path, d)| (path, d.start, d.end))
            .collect();
        assert_eq!(defined, [("cart.rs", 1, 3)]);

        let used: Vec<_> = index
            .references("total")
            .map(|(path, r)| (path, r.line))
            .collect();
        assert_eq!(used, [("shop.rs", 2)]);
        let caller = index.file("shop.rs").and_then(|s| s.enclosing(2));
        assert_eq!(caller.map(|d| d.name.as_str()), Some("pay"));
    }

    #[test]
    fn drops_removed_files_from_the_cache() {
        let mut tree = Tree::new();
        tree.write("a.rs", "fn a() {}\n", None);
        tree.write("b.rs", "fn b() {}\n", None);
        tree.update();

        tree.files.retain(|f| f.path != "b.rs");
        let (index, update) = tree.update();
        assert_eq!((update.files, update.parsed), (1, 0));
        assert_eq!(index.definitions("b").count(), 0);

        let cache = Cache::load(&tree.dir.path().join(INDEX_DIR).join(CACHE_FILE)).unwrap();
        assert_eq!(cache.symbols.len(), 1);
    }

    #[test]
    fn a_broken_cache_is_rebuilt() {
        let mut tree = Tree::new();
        tree.write("a.rs", "fn a() {}\n", None);
        tree.update();

        let dir = tree.dir.path().join(INDEX_DIR);
        fs::write(dir.join(CACHE_FILE), "not json").unwrap();
        let (_, update) = tree.update();
        assert_eq!((update.parsed, update.first), (1, true));
        assert_eq!(fs::read_to_string(dir.join(".gitignore")).unwrap(), "*\n");
    }
}
