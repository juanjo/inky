//! Files and folders handed to Inky by macOS (Finder, `open`, the `inky`
//! CLI). Requests that arrive before the webview is listening are queued and
//! drained once by the frontend.

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum OpenRequest {
    File { path: String },
    Folder { path: String },
}

/// `pending` is `Some` until the frontend drains it; afterwards requests are
/// emitted as events straight away.
pub struct OpenQueue {
    pending: Option<Vec<OpenRequest>>,
}

impl Default for OpenQueue {
    fn default() -> Self {
        OpenQueue { pending: Some(Vec::new()) }
    }
}

impl OpenQueue {
    pub fn push(&mut self, req: OpenRequest) -> Option<OpenRequest> {
        match self.pending.as_mut() {
            Some(list) => {
                list.push(req);
                None
            }
            None => Some(req),
        }
    }

    pub fn drain(&mut self) -> Vec<OpenRequest> {
        self.pending.take().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(p: &str) -> OpenRequest {
        OpenRequest::File { path: p.into() }
    }

    #[test]
    fn open_queue_holds_requests_until_drained() {
        let mut q = OpenQueue::default();
        assert!(q.push(file("/a.md")).is_none());
        assert!(q.push(file("/b.md")).is_none());
        assert_eq!(q.drain(), vec![file("/a.md"), file("/b.md")]);
    }

    #[test]
    fn open_queue_emits_directly_after_drain() {
        let mut q = OpenQueue::default();
        q.drain();
        assert_eq!(q.push(file("/c.md")), Some(file("/c.md")));
        assert!(q.drain().is_empty(), "nothing is delivered twice");
    }

    #[test]
    fn open_request_serializes_with_kind_tag() {
        let json = serde_json::to_string(&OpenRequest::Folder { path: "/x".into() }).unwrap();
        assert_eq!(json, r#"{"kind":"folder","path":"/x"}"#);
    }
}
