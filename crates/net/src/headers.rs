//! An HTTP header list: a case-insensitive multimap that keeps insertion
//! order.

/// A list of HTTP headers.
///
/// Header names are case-insensitive. They are stored in ASCII lowercase.
/// A name can occur more than once. The list keeps insertion order.
///
/// Values are strings. Values received from the network are decoded as
/// UTF-8 when they are valid UTF-8, otherwise as Latin-1 (each byte becomes
/// the code point with the same value).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers {
    entries: Vec<(String, String)>,
}

impl Headers {
    /// Creates an empty header list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the first value of the header `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.get_all(name).next()
    }

    /// Returns all values of the header `name`, in order.
    pub fn get_all<'a>(&'a self, name: &str) -> impl Iterator<Item = &'a str> {
        self.entries
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Returns true if the list contains the header `name`.
    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    /// Adds a header at the end of the list. Existing headers with the same
    /// name stay.
    pub fn append(&mut self, name: &str, value: impl Into<String>) {
        self.entries.push((name.to_ascii_lowercase(), value.into()));
    }

    /// Sets the header `name` to one value. The first existing header with
    /// this name keeps its position and gets the new value. Other headers
    /// with this name are removed. If there is no such header, the header is
    /// added at the end.
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self
            .entries
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            Some(index) => {
                self.entries[index].1 = value;
                let mut position = 0;
                self.entries.retain(|(n, _)| {
                    let keep = position <= index || !n.eq_ignore_ascii_case(name);
                    position += 1;
                    keep
                });
            }
            None => self.append(name, value),
        }
    }

    /// Removes all headers with the name `name`.
    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
    }

    /// Returns all headers as `(name, value)` pairs, in order. Names are in
    /// ASCII lowercase.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(n, v)| (n.as_str(), v.as_str()))
    }

    /// Returns the number of headers. A name with several values counts once
    /// per value.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the list is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<N: AsRef<str>, V: Into<String>> FromIterator<(N, V)> for Headers {
    fn from_iter<I: IntoIterator<Item = (N, V)>>(iter: I) -> Self {
        let mut headers = Headers::new();
        for (name, value) in iter {
            headers.append(name.as_ref(), value);
        }
        headers
    }
}

impl<N: AsRef<str>, V: Into<String>> Extend<(N, V)> for Headers {
    fn extend<I: IntoIterator<Item = (N, V)>>(&mut self, iter: I) {
        for (name, value) in iter {
            self.append(name.as_ref(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_case_insensitive() {
        let mut headers = Headers::new();
        headers.append("Content-Type", "text/html");
        assert_eq!(headers.get("content-type"), Some("text/html"));
        assert_eq!(headers.get("CONTENT-TYPE"), Some("text/html"));
        assert!(headers.contains("Content-type"));
        assert_eq!(headers.iter().next(), Some(("content-type", "text/html")));
    }

    #[test]
    fn append_keeps_all_values_in_order() {
        let mut headers = Headers::new();
        headers.append("Set-Cookie", "a=1");
        headers.append("X-Other", "x");
        headers.append("set-cookie", "b=2");
        assert_eq!(
            headers.get_all("SET-COOKIE").collect::<Vec<_>>(),
            ["a=1", "b=2"]
        );
        assert_eq!(headers.get("set-cookie"), Some("a=1"));
        assert_eq!(headers.len(), 3);
    }

    #[test]
    fn set_replaces_first_and_removes_others() {
        let mut headers: Headers = [("A", "1"), ("B", "2"), ("a", "3"), ("C", "4")]
            .into_iter()
            .collect();
        headers.set("A", "new");
        assert_eq!(
            headers.iter().collect::<Vec<_>>(),
            [("a", "new"), ("b", "2"), ("c", "4")]
        );
        headers.set("D", "5");
        assert_eq!(headers.get("d"), Some("5"));
        assert_eq!(headers.len(), 4);
    }

    #[test]
    fn remove_deletes_all_values() {
        let mut headers: Headers = [("A", "1"), ("B", "2"), ("a", "3")].into_iter().collect();
        headers.remove("a");
        assert_eq!(headers.iter().collect::<Vec<_>>(), [("b", "2")]);
        assert!(!headers.contains("A"));
        headers.remove("b");
        assert!(headers.is_empty());
    }
}
