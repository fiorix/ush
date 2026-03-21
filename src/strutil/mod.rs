use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub(crate) struct StringSet {
    inner: std::collections::HashSet<String>,
}

impl StringSet {
    pub(crate) fn new() -> Self {
        Self {
            inner: std::collections::HashSet::new(),
        }
    }

    pub(crate) fn from_file(path: &Path) -> io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let mut s = Self::new();
        for line in reader.lines() {
            let line = line?;
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            s.add(&line);
        }
        Ok(s)
    }

    pub(crate) fn add(&mut self, v: &str) {
        self.inner.insert(v.to_string());
    }

    pub(crate) fn remove(&mut self, v: &str) {
        self.inner.remove(v);
    }

    pub(crate) fn contains(&self, v: &str) -> bool {
        self.inner.contains(v)
    }

    pub(crate) fn sorted_strings(&self) -> Vec<String> {
        let mut v: Vec<String> = self.inner.iter().cloned().collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_string_set() {
        let mut s = StringSet::new();
        s.add("foo");
        s.add("foo");
        s.add("bar");
        s.add("baz");
        s.remove("bar");

        assert!(s.contains("foo"));
        assert_eq!(s.sorted_strings(), vec!["baz", "foo"]);
    }

    #[test]
    fn test_new_string_set_from_file() {
        let dir = std::env::temp_dir();
        let path = dir.join("ush-strutil-test");
        {
            let mut f = File::create(&path).unwrap();
            write!(f, "hello\nworld").unwrap();
        }

        let s = StringSet::from_file(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(s.sorted_strings(), vec!["hello", "world"]);
    }
}
