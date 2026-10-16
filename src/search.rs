//! Keyword search over downstream tools. BM25 over name + description + server,
//! with tool-name tokens weighted higher. No dependencies, fast enough for thousands of tools.

use std::collections::HashMap;

pub struct Doc {
    pub server: String,
    pub tool: String,
    pub description: String,
}

pub struct Index {
    docs: Vec<Doc>,
    terms: Vec<HashMap<String, f32>>,
    lengths: Vec<f32>,
    doc_freq: HashMap<String, usize>,
    avg_len: f32,
}

const NAME_WEIGHT: f32 = 3.0;
const K1: f32 = 1.2;
const B: f32 = 0.75;

pub fn tokenize(text: &str) -> Vec<String> {
    // Split on non-alphanumerics and camelCase boundaries, then lightly stem.
    let mut out = Vec::new();
    let mut word = String::new();
    let mut prev_lower = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if c.is_uppercase() && prev_lower && !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            prev_lower = c.is_lowercase();
            word.extend(c.to_lowercase());
        } else {
            prev_lower = false;
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out.into_iter().map(stem).filter(|w| w.len() > 1).collect()
}

/// Plural/gerund folding only, so "issues" and "issue" meet; never truncates short words.
fn stem(word: String) -> String {
    if word.len() > 4 {
        if let Some(base) = word.strip_suffix("ies") {
            return format!("{base}y");
        }
        if let Some(base) = word.strip_suffix("ing") {
            return base.to_string();
        }
    }
    if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        return word[..word.len() - 1].to_string();
    }
    word
}

impl Index {
    pub fn new(docs: Vec<Doc>) -> Self {
        let mut terms = Vec::with_capacity(docs.len());
        let mut lengths = Vec::with_capacity(docs.len());
        let mut doc_freq: HashMap<String, usize> = HashMap::new();
        for doc in &docs {
            let mut tf: HashMap<String, f32> = HashMap::new();
            for t in tokenize(&doc.tool).into_iter().chain(tokenize(&doc.server)) {
                *tf.entry(t).or_default() += NAME_WEIGHT;
            }
            for t in tokenize(&doc.description) {
                *tf.entry(t).or_default() += 1.0;
            }
            for t in tf.keys() {
                *doc_freq.entry(t.clone()).or_default() += 1;
            }
            lengths.push(tf.values().sum());
            terms.push(tf);
        }
        let avg_len = if lengths.is_empty() {
            1.0
        } else {
            lengths.iter().sum::<f32>() / lengths.len() as f32
        };
        Self {
            docs,
            terms,
            lengths,
            doc_freq,
            avg_len,
        }
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<&Doc> {
        let n = self.docs.len() as f32;
        let query = tokenize(query);
        let mut scored: Vec<(f32, usize)> = self
            .terms
            .iter()
            .enumerate()
            .filter_map(|(i, tf)| {
                let score: f32 = query
                    .iter()
                    .filter_map(|q| {
                        let f = *tf.get(q)?;
                        let df = *self.doc_freq.get(q)? as f32;
                        let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln();
                        let norm = K1 * (1.0 - B + B * self.lengths[i] / self.avg_len);
                        Some(idf * f * (K1 + 1.0) / (f + norm))
                    })
                    .sum();
                (score > 0.0).then_some((score, i))
            })
            .collect();
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, i)| &self.docs[i])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(server: &str, tool: &str, description: &str) -> Doc {
        Doc {
            server: server.into(),
            tool: tool.into(),
            description: description.into(),
        }
    }

    #[test]
    fn tokenizes_snake_and_camel() {
        assert_eq!(
            tokenize("createPullRequest"),
            vec!["create", "pull", "request"]
        );
        assert_eq!(tokenize("list_issues"), vec!["list", "issue"]);
        assert_eq!(
            tokenize("issue repositories access"),
            vec!["issue", "repository", "access"]
        );
    }

    #[test]
    fn ranks_name_matches_first() {
        let index = Index::new(vec![
            doc(
                "github",
                "create_issue",
                "Create a new issue in a repository",
            ),
            doc(
                "github",
                "list_repos",
                "List repositories, including issue counts",
            ),
            doc("slack", "send_message", "Send a message to a channel"),
        ]);
        let hits = index.search("open an issue", 2);
        assert_eq!(hits[0].tool, "create_issue");
        assert_eq!(
            index.search("post slack message", 1)[0].tool,
            "send_message"
        );
        assert!(index.search("kubernetes", 5).is_empty());
    }
}
