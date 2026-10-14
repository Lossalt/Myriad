//! Glancing at talk that did not call her, and her judgment of whether to
//! speak up or only react.

use super::*;

pub(crate) fn look_loop(cx: &mut Cx, mut errand: Errand) {
    let now = cx.now();
    let Kind::Look { id, .. } = &errand.kind else {
        return;
    };
    let Some(pending) = cx.group.pending.as_ref() else {
        return;
    };
    let latest = pending.message_id == *id;
    let overdue = cx
        .group
        .unjudged_since
        .is_some_and(|since| now - since >= seconds(MAX_WAIT_SECONDS));
    if !latest && !overdue {
        return;
    }
    if cx.group.judging || cx.group.busy {
        let key = cx.wake(SETTLE_SECONDS);
        errand.doing = Doing::Settling;
        park_errand(cx, errand, key);
        return;
    }
    cx.group.judging = true;
    cx.group.unjudged_since = None;
    let Some(message) = cx.group.pending.take() else {
        return;
    };
    if let Kind::Look { message: held, .. } = &mut errand.kind {
        *held = Some(message.clone());
    }
    let mut judging = Box::new(Judging {
        message,
        stage: JudgeStage::Identifying,
        owner: None,
    });
    match judge_start(cx, &mut judging) {
        Ok(awaiting) => {
            errand.doing = Doing::Judging(judging);
            park_errand(cx, errand, awaiting);
        }
        Err(decided) => judged(cx, errand, decided),
    }
}

pub(crate) fn judge_start(cx: &mut Cx, judging: &mut Judging) -> Result<String, Option<Decision>> {
    let now = cx.now();
    if stopped_answering(cx.group, &judging.message, now) || muted_now(cx.group, now) {
        return Err(None);
    }
    judging.stage = JudgeStage::Identifying;
    let from = judging.message.from.clone();
    Ok(cx.effect(|id| Effect::Identify { id, from }))
}

/// The pictures in the recent talk she has not seen yet.
pub(crate) fn unseen(group: &Group) -> Vec<Picture> {
    let lines = group.lines.len();
    group
        .lines
        .iter()
        .skip(lines.saturating_sub(CONVERSATION_LINES))
        .filter(|line| !line.hers)
        .flat_map(|line| {
            line.images
                .iter()
                .enumerate()
                .filter(|(index, _)| line.seen.get(*index).is_none_or(Option::is_none))
                .map(|(index, image)| Picture {
                    message_id: line.message_id.clone(),
                    index,
                    image: image.clone(),
                })
                .collect::<Vec<_>>()
        })
        .take(PICTURES_AT_ONCE)
        .collect()
}

/// What she saw, put on the lines; a picture sent there again is the
/// group's.
pub(crate) fn apply_seen(cx: &mut Cx, looked: Vec<Looked>) {
    for looked in looked {
        let key = cx
            .group
            .lines
            .iter()
            .find(|line| !line.hers && line.message_id == looked.message_id)
            .and_then(|line| line.images.get(looked.index))
            .map(|image| image.key.clone());
        if let Some(key) = key
            && cx.group.pictures.get(&key).is_some_and(|sent| *sent >= 2)
        {
            cx.out.effects.push(Effect::PictureAgain {
                key,
                seen: looked.seen.clone(),
            });
        }
        if let Some(line) = cx
            .group
            .lines
            .iter_mut()
            .find(|line| !line.hers && line.message_id == looked.message_id)
        {
            if line.seen.len() <= looked.index {
                line.seen.resize(looked.index + 1, None);
            }
            line.seen[looked.index] = Some(looked.seen);
        }
    }
}

pub(crate) fn judge_step(
    cx: &mut Cx,
    judging: &mut Judging,
    answer: Option<Answer>,
) -> Result<String, Option<Decision>> {
    match (judging.stage, answer) {
        (JudgeStage::Identifying, Some(Answer::Identified { who, owner })) => {
            let owner = match who {
                Who::Member { user_id, bound } => {
                    if !bound {
                        return Err(None);
                    }
                    user_id
                }
                Who::Stranger | Who::Unknown => owner.ok_or(None)?,
            };
            judging.owner = Some(owner);
            let pictures = unseen(cx.group);
            if pictures.is_empty() {
                return Ok(judge_ask(cx, judging));
            }
            judging.stage = JudgeStage::Looking;
            Ok(cx.effect(|id| Effect::Look { id, pictures }))
        }
        (JudgeStage::Looking, Some(Answer::Looked { seen })) => {
            apply_seen(cx, seen);
            Ok(judge_ask(cx, judging))
        }
        (JudgeStage::Judging, Some(Answer::Judged { decision })) => Err(decision),
        _ => Err(None),
    }
}

pub(crate) fn judge_ask(cx: &mut Cx, judging: &mut Judging) -> String {
    let now = cx.now();
    let lines = transcript(cx.group, None, now);
    let conversation = conversation(&lines);
    let late = cx
        .group
        .lines
        .iter()
        .rev()
        .find(|line| !line.hers)
        .map(|line| age(now, line.at_micros()))
        .filter(|age| *age >= chrono::Duration::minutes(LATE_MINUTES))
        .map(crate::doing::ago_text);
    let last_spoke = cx
        .group
        .lines
        .iter()
        .rev()
        .find(|line| line.hers)
        .map(|line| crate::doing::ago_text(age(now, line.at_micros())));
    let how_it_went = crate::joining::how_it_went(
        cx.group.spoke_up.len(),
        cx.group.spoke_up.iter().filter(|one| one.taken).count(),
    );
    let share = {
        let recent: Vec<&Line> = cx
            .group
            .lines
            .iter()
            .skip(cx.group.lines.len().saturating_sub(CONVERSATION_LINES))
            .collect();
        let hers = recent.iter().filter(|line| line.hers).count();
        let others = recent
            .iter()
            .filter(|line| !line.hers)
            .map(|line| line.from.as_deref().unwrap_or(&line.name))
            .collect::<std::collections::HashSet<_>>()
            .len();
        crate::joining::share_of_talk(recent.len(), hers, others)
    };
    judging.stage = JudgeStage::Judging;
    let owner = judging.owner.unwrap_or_default();
    cx.effect(|id| Effect::Judge {
        id,
        owner,
        conversation,
        here: Here {
            last_spoke,
            late,
            how_it_went,
            share,
        },
    })
}

pub(crate) fn judged(cx: &mut Cx, mut errand: Errand, decided: Option<Decision>) {
    let now = cx.now();
    cx.group.judging = false;
    // Going on with what she said, without calling her: taken up.
    if decided
        .as_ref()
        .is_some_and(|decided| decided.why == Why::Answer)
    {
        taken_up(cx.group, now);
    }
    let Kind::Look {
        message,
        why,
        spoke,
        ..
    } = &mut errand.kind
    else {
        return;
    };
    let Some(target) = message.clone() else {
        chain_end(cx, errand);
        return;
    };
    match decided {
        // No words, only a reaction on the line.
        Some(Decision {
            why: Why::React,
            reason,
        }) => {
            let to = target.target();
            let emoji = reason.clone();
            let awaiting = cx.effect(|id| Effect::React { id, to, emoji });
            errand.doing = Doing::Reacting { emoji: reason };
            park_errand(cx, errand, awaiting);
        }
        Some(decided) => {
            if begin(cx.group, now).is_ok() {
                *why = Some(decided.why);
                *spoke = true;
                start_answer(cx, errand, target, Some(decided.reason), false);
            } else {
                chain_end(cx, errand);
            }
        }
        None => chain_end(cx, errand),
    }
}
