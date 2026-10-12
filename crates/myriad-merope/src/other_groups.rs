//! What she brings into one group from the other groups she is in: the same
//! person met there, what they said about themselves there, and later that
//! group's bits and days. Each says which group, by its name, so she knows
//! the people here may not have been there; whether she brings it up is hers.

use myriad_agent_rules::untrusted_block;

/// Something from another group: the group's name (`None` when she does not
/// have it) and what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromGroup {
    pub group: Option<String>,
    pub text: String,
}

/// Where, as told: by the group's name when there is one.
pub fn in_group(group: Option<&str>) -> String {
    match group {
        Some(name) => format!("in the group 「{name}」"),
        None => "in a group".to_string(),
    }
}

fn lines(from: &[FromGroup]) -> String {
    from.iter()
        .map(|from| format!("- {}: {}", in_group(from.group.as_deref()), from.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Someone from the community she also knows from other groups: which ones,
/// and what they said about themselves there that the talk brings back.
pub fn they_section(groups: &[Option<String>], said: &[FromGroup]) -> Option<String> {
    if groups.is_empty() && said.is_empty() {
        return None;
    }
    let mut text = String::from("## Them, in other groups\n");
    if !groups.is_empty() {
        let named: Vec<String> = groups
            .iter()
            .map(|group| in_group(group.as_deref()))
            .collect();
        text.push_str(&format!("You also talk with them {}.", named.join(", ")));
    }
    if !said.is_empty() {
        if !groups.is_empty() {
            text.push(' ');
        }
        text.push_str(
            "What they said about themselves in other groups that this talk brings back; the people here may not have been there:\n",
        );
        text.push_str(&untrusted_block("said_in_other_groups", &lines(said)));
    }
    Some(text)
}

/// Her notes on the same account in other groups, for someone from outside.
pub fn noted_elsewhere(notes: &[FromGroup]) -> Option<String> {
    (!notes.is_empty()).then(|| {
        format!(
            "You know them from other groups too, as the same account there. What you noted of them in each:\n{}",
            untrusted_block("noted_in_other_groups", &lines(notes))
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_comes_from_another_group_says_which() {
        assert_eq!(they_section(&[], &[]), None);
        let block = they_section(
            &[Some("摸鱼小队".into()), None],
            &[FromGroup {
                group: Some("摸鱼小队".into()),
                text: "下个月要去大阪".into(),
            }],
        )
        .unwrap();
        assert!(block.contains("You also talk with them in the group 「摸鱼小队」, in a group."));
        assert!(block.contains("- in the group 「摸鱼小队」: 下个月要去大阪"));
        assert!(block.contains("may not have been there"));
        assert_eq!(noted_elsewhere(&[]), None);
        assert!(
            noted_elsewhere(&[FromGroup {
                group: None,
                text: "叫阿木，喜欢猫".into(),
            }])
            .unwrap()
            .contains("- in a group: 叫阿木，喜欢猫")
        );
    }
}
