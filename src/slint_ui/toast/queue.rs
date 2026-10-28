use std::collections::VecDeque;
use std::time::Duration;

pub(crate) const TOAST_LIFETIME: Duration = Duration::from_secs(4);

const MAX_VISIBLE_TOASTS: usize = 3;

// User-facing toast notice kept independent from Slint generated types.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToastNotice {
    pub text: String,
    pub level: i32,
    pub closing: bool,
}

// Internal queue entry, kept as a flat structure.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToastEntry {
    pub id: u32,
    pub text: String,
    pub level: i32,
    pub closing: bool,
}

#[derive(Default)]
pub struct ToastQueue {
    pub(crate) stack: VecDeque<ToastEntry>,
    next_id: u32,
}

impl ToastQueue {
    pub fn push_toast(&mut self, notice: ToastNotice) -> (u32, Option<u32>) {
        let evicted_id = if self.active_toast_count() >= MAX_VISIBLE_TOASTS {
            self.oldest_active_toast_id()
        } else {
            None
        };

        let eviction_started = if let Some(evicted_id) = evicted_id {
            self.begin_toast_closing(evicted_id)
        } else {
            false
        };

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.stack.push_back(ToastEntry {
            id,
            text: notice.text,
            level: notice.level,
            closing: notice.closing,
        });

        let evicted = if eviction_started { evicted_id } else { None };
        (id, evicted)
    }

    #[allow(dead_code)]
    pub fn remove_toast(&mut self, id: u32) -> bool {
        if let Some(index) = self.stack.iter().position(|toast| toast.id == id) {
            self.stack.remove(index);
            true
        } else {
            false
        }
    }

    pub fn begin_toast_closing(&mut self, id: u32) -> bool {
        if let Some(toast) = self.stack.iter_mut().find(|toast| toast.id == id) {
            if toast.closing {
                false
            } else {
                toast.closing = true;
                true
            }
        } else {
            false
        }
    }

    pub fn finalize_toast_removal(&mut self, id: u32) -> bool {
        if let Some(index) = self.stack.iter().position(|toast| toast.id == id) {
            if self.stack[index].closing {
                self.stack.remove(index);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn active_toast_count(&self) -> usize {
        self.stack.iter().filter(|toast| !toast.closing).count()
    }

    pub fn oldest_active_toast_id(&self) -> Option<u32> {
        self.stack
            .iter()
            .find(|toast| !toast.closing)
            .map(|toast| toast.id)
    }

    #[allow(dead_code)]
    pub fn visible_toasts(&self) -> Vec<ToastNotice> {
        self.stack
            .iter()
            .map(|toast| ToastNotice {
                text: toast.text.clone(),
                level: toast.level,
                closing: toast.closing,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{ToastNotice, ToastQueue};

    #[test]
    fn toast_stack_stores_oldest_to_newest_notices_and_ids() {
        let mut queue = ToastQueue::default();
        let (first, _) = queue.push_toast(ToastNotice {
            text: "first".to_owned(),
            level: 0,
            closing: false,
        });
        let (second, _) = queue.push_toast(ToastNotice {
            text: "second".to_owned(),
            level: 1,
            closing: false,
        });

        assert_eq!(queue.stack.len(), 2);
        assert_eq!(
            queue
                .visible_toasts()
                .iter()
                .map(|item| item.text.clone())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert_eq!(
            queue.stack.iter().map(|toast| toast.id).collect::<Vec<_>>(),
            vec![first, second]
        );
        assert_ne!(first, second);
    }

    #[test]
    fn toast_stack_begins_animated_eviction_at_three() {
        let mut queue = ToastQueue::default();
        let (first, _) = queue.push_toast(ToastNotice {
            text: "first".to_owned(),
            level: 0,
            closing: false,
        });
        let (second, _) = queue.push_toast(ToastNotice {
            text: "second".to_owned(),
            level: 0,
            closing: false,
        });
        let (third, _) = queue.push_toast(ToastNotice {
            text: "third".to_owned(),
            level: 0,
            closing: false,
        });
        let (fourth, evicted) = queue.push_toast(ToastNotice {
            text: "fourth".to_owned(),
            level: 0,
            closing: false,
        });

        assert_eq!(evicted, Some(first));
        assert_eq!(queue.stack.len(), 4);
        assert_eq!(
            queue
                .visible_toasts()
                .iter()
                .map(|item| item.text.clone())
                .collect::<Vec<_>>(),
            vec!["first", "second", "third", "fourth"]
        );
        assert!(queue.stack[0].closing);
        assert!(!queue.stack[1].closing);
        assert!(!queue.stack[2].closing);
        assert!(!queue.stack[3].closing);
        assert_eq!(
            queue.stack.iter().map(|toast| toast.id).collect::<Vec<_>>(),
            vec![first, second, third, fourth]
        );
        assert_eq!(third, 2);
        assert_eq!(fourth, 3);
        assert!(!queue.begin_toast_closing(first));
        assert_eq!(queue.stack.len(), 4);
        assert!(queue.finalize_toast_removal(first));
        assert_eq!(queue.stack.len(), 3);
        assert_eq!(
            queue
                .visible_toasts()
                .iter()
                .map(|item| item.text.clone())
                .collect::<Vec<_>>(),
            vec!["second", "third", "fourth"]
        );
        assert_eq!(
            queue.stack.iter().map(|toast| toast.id).collect::<Vec<_>>(),
            vec![second, third, fourth]
        );
    }

    #[test]
    fn toast_close_is_two_phase() {
        let mut queue = ToastQueue::default();
        let (id, _) = queue.push_toast(ToastNotice {
            text: "first".to_owned(),
            level: 2,
            closing: false,
        });

        assert!(queue.begin_toast_closing(id));
        assert_eq!(queue.stack.len(), 1);
        assert!(queue.stack[0].closing);
        assert!(!queue.begin_toast_closing(id));
        assert!(!queue.finalize_toast_removal(id.wrapping_add(1)));
        assert!(queue.finalize_toast_removal(id));
        assert!(queue.stack.is_empty());
    }

    #[test]
    fn toast_stack_removes_only_requested_id() {
        let mut queue = ToastQueue::default();
        let (first, _) = queue.push_toast(ToastNotice {
            text: "first".to_owned(),
            level: 0,
            closing: false,
        });
        let (_second, _) = queue.push_toast(ToastNotice {
            text: "second".to_owned(),
            level: 0,
            closing: false,
        });

        assert!(queue.remove_toast(first));
        assert_eq!(queue.stack.len(), 1);
        assert_eq!(queue.stack[0].text, "second");
    }

    #[test]
    fn toast_stack_ignores_unknown_ids() {
        let mut queue = ToastQueue::default();
        let (id, _) = queue.push_toast(ToastNotice {
            text: "first".to_owned(),
            level: 2,
            closing: false,
        });
        let expected_stack = queue.visible_toasts();

        assert!(!queue.remove_toast(id.wrapping_add(1)));
        assert!(!queue.remove_toast(u32::MAX));
        assert_eq!(queue.visible_toasts(), expected_stack);
    }
}
