//! Handing work off from chat, over IM.
//!
//! In a private IM conversation she talks as herself (Chat). When they ask
//! her to get something done, she hands it off: she says so, and puts one
//! `[[work: …]]` line at the end with what to do. The channel then starts an
//! ordinary Work run for them with that instruction, as if they had asked for
//! it themselves: the same granted permissions, the same confirmations and
//! questions, delivered the same way. Her own ideas she asks about first.
//!
//! The next time they talk she knows what she handed off and how it came back.
//! Only a channel sets this up (`RequestContext::channel_chat`, never from a
//! web request); the web panel keeps its own two modes.

use crate::services::agent::types::ChannelChat;

const OPEN: &str = "[[work:";
const CLOSE: &str = "]]";
const MAX_INSTRUCTION_CHARS: usize = 500;

/// Take her hand-off lines out of a reply: the text without them, and the
/// last instruction she gave, if any.
pub fn split(raw: &str) -> (String, Option<String>) {
    let mut text = raw.to_string();
    let mut instruction = None;
    while let Some(start) = text.find(OPEN) {
        let after = start + OPEN.len();
        let Some(close) = text[after..].find(CLOSE) else {
            break;
        };
        let inner: String = text[after..after + close]
            .trim()
            .chars()
            .take(MAX_INSTRUCTION_CHARS)
            .collect();
        if !inner.is_empty() {
            instruction = Some(inner);
        }
        text.replace_range(start..after + close + CLOSE.len(), "");
    }
    (text.trim_end().to_string(), instruction)
}

/// What she knows about handing work off, for a private IM chat.
pub fn section(chat: &ChannelChat) -> String {
    let mut lines = vec![
        "## Getting things done".to_string(),
        "You cannot use tools yourself in this chat, but a helper can: looking things up on the web, working with their data on this site, making or changing things for them. \
When they ask you to get something like that done, say in a few words that you are on it, and put [[work: what exactly to do, in one line, keeping their own words]] on its own last line. \
The helper does it with their permissions; any question or confirmation it needs goes to them directly, and the result comes back to them. \
If it is your own idea, ask first and hand it off only after they say yes. Do not hand off plain conversation. Do not read that line aloud."
            .to_string(),
    ];
    if chat.busy {
        lines.push(
            "Something you handed off is still being done: do not hand off anything new until it is back; say it is still on it."
                .to_string(),
        );
    }
    if let Some(handed_off) = chat.handed_off.as_deref() {
        lines.push(myriad_agent_rules::untrusted_block(
            "handed_off",
            handed_off,
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn her_hand_off_line_is_taken_out_of_what_she_says() {
        assert_eq!(
            split("好，我让助手去查。\n[[work: 查一下明天东京的天气]]"),
            (
                "好，我让助手去查。".into(),
                Some("查一下明天东京的天气".into())
            )
        );
        assert_eq!(split("今天好累"), ("今天好累".into(), None));
        assert_eq!(split("嗯[[work:   ]]"), ("嗯".into(), None));
        // An unfinished line is left for the stream to hold back.
        assert_eq!(split("嗯[[work: 查"), ("嗯[[work: 查".into(), None));
    }

    #[test]
    fn she_hands_off_only_what_they_asked_for() {
        let section = section(&ChannelChat::default());
        assert!(section.contains("If it is your own idea, ask first"));
        assert!(section.contains("with their permissions"));
        assert!(!section.contains("still being done"));
        let busy = super::section(&ChannelChat {
            busy: true,
            handed_off: Some("You handed off: 查天气".into()),
        });
        assert!(busy.contains("still being done"));
        assert!(busy.contains("handed_off"));
    }
}
