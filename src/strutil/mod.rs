use std::collections::HashSet;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct StringSet {
    inner: HashSet<String>,
}

impl StringSet {
    pub fn new() -> Self {
        Self {
            inner: HashSet::new(),
        }
    }

    #[allow(dead_code)]
    pub fn from_slice(values: &[&str]) -> Self {
        let mut s = Self::new();
        for v in values {
            s.add(v);
        }
        s
    }

    pub fn from_file(path: &Path) -> io::Result<Self> {
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

    pub fn add(&mut self, v: &str) {
        self.inner.insert(v.to_string());
    }

    pub fn remove(&mut self, v: &str) {
        self.inner.remove(v);
    }

    pub fn contains(&self, v: &str) -> bool {
        self.inner.contains(v)
    }

    pub fn sorted_strings(&self) -> Vec<String> {
        let mut v: Vec<String> = self.inner.iter().cloned().collect();
        v.sort();
        v
    }
}

#[allow(dead_code)]
pub fn unique(s: &[String]) -> Vec<String> {
    let set: HashSet<&String> = s.iter().collect();
    let mut v: Vec<String> = set.into_iter().cloned().collect();
    v.sort();
    v
}

#[allow(dead_code)]
pub fn merge(lists: &[&[String]]) -> StringSet {
    let mut set = StringSet::new();
    for list in lists {
        for item in *list {
            set.add(item);
        }
    }
    set
}

#[allow(dead_code)]
pub fn intersect(lists: &[&[String]]) -> StringSet {
    if lists.is_empty() {
        return StringSet::new();
    }
    let sets: Vec<HashSet<&String>> = lists
        .iter()
        .map(|list| list.iter().collect::<HashSet<&String>>())
        .collect();

    let mut result = StringSet::new();
    for item in &sets[0] {
        if sets.iter().all(|s| s.contains(item)) {
            result.add(item);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_string_set() {
        let mut s = StringSet::from_slice(&["foo", "foo", "bar"]);
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

    #[test]
    fn test_unique() {
        let have = unique(&["foo".to_string(), "foo".to_string()]);
        assert_eq!(have, vec!["foo"]);
    }

    #[test]
    fn test_merge() {
        let a = vec!["foo".to_string(), "bar".to_string()];
        let b = vec!["foo".to_string(), "baz".to_string()];
        let have = merge(&[&a, &b]).sorted_strings();
        assert_eq!(have, vec!["bar", "baz", "foo"]);
    }

    #[test]
    fn test_intersect() {
        let a = vec!["foo".to_string(), "bar".to_string()];
        let b = vec!["foo".to_string(), "baz".to_string()];
        let have = intersect(&[&a, &b]).sorted_strings();
        assert_eq!(have, vec!["foo"]);
    }
}
