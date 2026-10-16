//! 阿瓦隆 (The Resistance: Avalon), as a game she hosts at a group's table:
//! five to ten players, all of it mechanical here, the referee's part.
//!
//! Each seat is whispered its role and whom it knows: Merlin the evil (all
//! but Mordred), Percival Merlin and Morgana (not which is which), the evil
//! each other (all but Oberon, who knows nobody and whom nobody knows). Five
//! quests: for each, the leader picks the team in private, one by one; then
//! everyone votes for it or against it in private, and how each voted is
//! told. More than half for it, the team goes; otherwise the next leader
//! picks, and the fifth team in a row turned down is a win for the evil.
//! Those on a team each play a card in private: the good only success, the
//! evil success or fail; one fail fails the quest (two on the fourth with
//! seven or more). Three quests one way end it, but when the good win three
//! the assassin guesses who Merlin is, and a right guess wins it for the
//! evil. At the end every role is told.
//!
//! No model is asked for any rule: only for her lines as the game's voice,
//! and her seat's moves when she plays one (from her seat's view alone).

use serde::{Deserialize, Serialize};

use crate::group::Rolls;
use crate::hosting::{Fact, Rules, Seat, Waiting};

pub const KIND: &str = "avalon";
pub const NAME: &str = "阿瓦隆";
pub const FEWEST: usize = 5;
pub const MOST: usize = 10;
/// How long the leader has for each pick, everyone to vote, a team to play.
pub const SECONDS: f64 = 60.0;
/// Teams turned down in a row that win it for the evil.
pub const REJECTIONS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Merlin,
    Percival,
    Loyal,
    Morgana,
    Assassin,
    Mordred,
    Oberon,
    Minion,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Merlin => "梅林",
            Role::Percival => "派西维尔",
            Role::Loyal => "亚瑟的忠臣",
            Role::Morgana => "莫甘娜",
            Role::Assassin => "刺客",
            Role::Mordred => "莫德雷德",
            Role::Oberon => "奥伯伦",
            Role::Minion => "莫德雷德的爪牙",
        }
    }

    pub fn evil(self) -> bool {
        matches!(
            self,
            Role::Morgana | Role::Assassin | Role::Mordred | Role::Oberon | Role::Minion
        )
    }
}

/// The roles for `players` (five to ten).
pub fn board(players: usize) -> Vec<Role> {
    use Role::*;
    let (loyal, evil): (usize, &[Role]) = match players {
        0..=5 => (1, &[Morgana, Assassin]),
        6 => (2, &[Morgana, Assassin]),
        7 => (2, &[Morgana, Assassin, Oberon]),
        8 => (3, &[Morgana, Assassin, Minion]),
        9 => (4, &[Mordred, Morgana, Assassin]),
        _ => (4, &[Mordred, Morgana, Assassin, Oberon]),
    };
    let mut roles = vec![Merlin, Percival];
    roles.extend(vec![Loyal; loyal]);
    roles.extend_from_slice(evil);
    roles
}

/// How many go on each of the five quests, with `players`.
pub fn team_sizes(players: usize) -> [usize; 5] {
    match players {
        0..=5 => [2, 3, 2, 3, 3],
        6 => [2, 3, 4, 3, 4],
        7 => [2, 3, 3, 4, 4],
        _ => [3, 4, 4, 5, 5],
    }
}

/// Fails that fail quest `quest` (0-based), with `players`.
pub fn fails_needed(players: usize, quest: usize) -> usize {
    if players >= 7 && quest == 3 { 2 } else { 1 }
}

/// What the game is doing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "phase")]
pub enum Phase {
    /// The leader picks the team, one by one.
    Pick,
    /// Everyone votes for the team or against it (`true` for it).
    #[serde(rename_all = "camelCase")]
    Vote {
        #[serde(default)]
        votes: Vec<(usize, bool)>,
    },
    /// The team each play a card (`true` success).
    #[serde(rename_all = "camelCase")]
    Quest {
        #[serde(default)]
        cards: Vec<(usize, bool)>,
    },
    /// The good won three: the assassin guesses who Merlin is.
    Assassinate,
    #[serde(rename_all = "camelCase")]
    Over { evil_won: bool },
}

/// A game of 阿瓦隆.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Avalon {
    pub seats: Vec<Seat>,
    pub roles: Vec<Role>,
    pub leader: usize,
    /// How each quest went so far (`true` succeeded).
    pub results: Vec<bool>,
    /// Teams turned down in a row on this quest.
    pub rejections: u32,
    pub team: Vec<usize>,
    pub phase: Phase,
    /// Everything the game holds, each with whom it is for, in order.
    pub log: Vec<Fact>,
    pub moments: usize,
    /// Which wait it is (one more each time the game moves on).
    pub phases: u64,
}

const FOR: &str = "赞成";
const AGAINST: &str = "反对";
const SUCCESS: &str = "成功";
const FAIL: &str = "失败";

impl Avalon {
    /// Dealt to `seats`: the roles and the first leader, by `rolls`.
    pub fn deal(seats: Vec<Seat>, rolls: &mut Rolls) -> Self {
        let players = seats.len();
        let mut left = board(players);
        let mut roles = Vec::with_capacity(players);
        while !left.is_empty() {
            let pick = rolls.range(0..=(left.len() as u64 - 1)) as usize;
            roles.push(left.remove(pick));
        }
        let leader = rolls.range(0..=(players.max(1) as u64 - 1)) as usize;
        let mut game = Self {
            seats,
            roles,
            leader,
            results: Vec::new(),
            rejections: 0,
            team: Vec::new(),
            phase: Phase::Pick,
            log: Vec::new(),
            moments: 1,
            phases: 1,
        };
        let names: Vec<&str> = game.seats.iter().map(|seat| seat.name.as_str()).collect();
        let evil = game.roles.iter().filter(|role| role.evil()).count();
        let mut cast: Vec<&str> = board(players).iter().map(|role| role.name()).collect();
        cast.dedup();
        let line = format!(
            "{NAME}：{} 人（{}），好人 {} 个、坏人 {} 个，角色有{}。{}",
            players,
            names.join("、"),
            players - evil,
            evil,
            cast.join("、"),
            told_rules(players)
        );
        game.log.push(Fact::all(line));
        for seat in 0..players {
            let line = game.knows(seat);
            game.log.push(Fact::seat(seat, line));
        }
        game.ask_leader();
        game
    }

    fn name(&self, seat: usize) -> &str {
        &self.seats[seat].name
    }

    fn names(&self, seats: &[usize]) -> String {
        seats
            .iter()
            .map(|seat| self.name(*seat))
            .collect::<Vec<_>>()
            .join("、")
    }

    fn with(&self, roles: &dyn Fn(Role) -> bool) -> Vec<usize> {
        (0..self.seats.len())
            .filter(|seat| roles(self.roles[*seat]))
            .collect()
    }

    /// What `seat` is whispered at the start: its role, and whom it knows.
    fn knows(&self, seat: usize) -> String {
        let role = self.roles[seat];
        let side = if role.evil() { "坏人" } else { "好人" };
        let mut line = format!("你的身份：{}（{side}）。", role.name());
        match role {
            Role::Merlin => {
                let seen = self.with(&|other| other.evil() && other != Role::Mordred);
                line.push_str(&format!("你看到的坏人：{}。", self.names(&seen)));
                if self.roles.contains(&Role::Mordred) {
                    line.push_str("莫德雷德也是坏人，但你看不到他。");
                }
                line.push_str("别让坏人认出你：好人赢下三个任务后，刺客会刺杀一次梅林。");
            }
            Role::Percival => {
                let seen = self.with(&|other| matches!(other, Role::Merlin | Role::Morgana));
                line.push_str(&format!(
                    "{}里一个是梅林、一个是莫甘娜，你不知道谁是谁。",
                    self.names(&seen)
                ));
            }
            Role::Loyal => {}
            Role::Oberon => line.push_str("你不知道同伴是谁，他们也不知道你。"),
            _ => {
                let mates: Vec<usize> = (0..self.seats.len())
                    .filter(|other| {
                        *other != seat
                            && self.roles[*other].evil()
                            && self.roles[*other] != Role::Oberon
                    })
                    .collect();
                line.push_str(&format!("你的坏人同伴：{}。", self.names(&mates)));
                if self.roles.contains(&Role::Oberon) {
                    line.push_str("奥伯伦也是坏人，但你们互相不知道。");
                }
                line.push_str(match role {
                    Role::Assassin => "好人赢下三个任务后，由你刺杀梅林，刺中坏人赢。",
                    Role::Morgana => "派西维尔会把你和梅林一起看到，分不清谁是谁。",
                    Role::Mordred => "梅林看不到你。",
                    _ => "",
                });
            }
        }
        line
    }

    fn size(&self) -> usize {
        team_sizes(self.seats.len())[self.results.len().min(4)]
    }

    fn ask_leader(&mut self) {
        self.team.clear();
        self.phases += 1;
        self.phase = Phase::Pick;
        let line = format!(
            "第{}个任务要 {} 人，{}当队长{}：私聊选出任务的人。",
            self.results.len() + 1,
            self.size(),
            self.name(self.leader),
            match self.rejections {
                0 => String::new(),
                turned => format!(
                    "（这个任务已经 {turned} 次没组成队，第 {REJECTIONS} 次也没组成坏人赢）"
                ),
            }
        );
        self.log.push(Fact::all(line));
    }

    fn next_leader(&mut self) {
        self.leader = (self.leader + 1) % self.seats.len().max(1);
    }

    /// The team is not made: the next leader picks, or that was the last.
    fn turned_down(&mut self) {
        self.rejections += 1;
        if self.rejections >= REJECTIONS {
            self.log
                .push(Fact::all(format!("连续 {REJECTIONS} 次没组成队。")));
            self.end(true);
            return;
        }
        self.next_leader();
        self.ask_leader();
    }

    /// The votes on the team counted (who did not vote is not for it).
    fn count(&mut self) {
        let Phase::Vote { votes } = &self.phase else {
            return;
        };
        let votes = votes.clone();
        let of = |want: Option<bool>| -> Vec<usize> {
            (0..self.seats.len())
                .filter(|seat| {
                    votes
                        .iter()
                        .find(|(by, _)| by == seat)
                        .map(|(_, vote)| *vote)
                        == want
                })
                .collect()
        };
        let (yes, no, none) = (of(Some(true)), of(Some(false)), of(None));
        let mut tally = vec![format!(
            "赞成 {}",
            if yes.is_empty() {
                "没有".into()
            } else {
                self.names(&yes)
            }
        )];
        tally.push(format!(
            "反对 {}",
            if no.is_empty() {
                "没有".into()
            } else {
                self.names(&no)
            }
        ));
        if !none.is_empty() {
            tally.push(format!("没投 {}", self.names(&none)));
        }
        let passed = yes.len() * 2 > self.seats.len();
        self.log.push(Fact::all(format!(
            "投票：{}。{}",
            tally.join("；"),
            if passed {
                "过半赞成，出发：出任务的人私聊出牌。"
            } else {
                "没过半，这次没组成队。"
            }
        )));
        if passed {
            self.phases += 1;
            self.phase = Phase::Quest { cards: Vec::new() };
        } else {
            self.turned_down();
        }
    }

    /// The cards played (who did not play counts as success, unsaid).
    fn reveal(&mut self) {
        let Phase::Quest { cards } = &self.phase else {
            return;
        };
        let fails = cards.iter().filter(|(_, success)| !success).count();
        let quest = self.results.len();
        let needed = fails_needed(self.seats.len(), quest);
        let success = fails < needed;
        self.results.push(success);
        self.moments += 1;
        let done = |want: bool| self.results.iter().filter(|known| **known == want).count();
        self.log.push(Fact::all(format!(
            "第{}个任务：{} 张牌里 {} 张失败，任务{}。现在好人 {} 胜、坏人 {} 胜。",
            quest + 1,
            self.team.len(),
            fails,
            if success { "成功" } else { "失败" },
            done(true),
            done(false)
        )));
        if done(false) >= 3 {
            self.end(true);
        } else if done(true) >= 3 {
            self.phases += 1;
            self.phase = Phase::Assassinate;
            self.log.push(Fact::all(
                "好人赢下三个任务。刺客还有一次机会：私聊选出谁是梅林，刺中坏人赢。".to_string(),
            ));
        } else {
            self.rejections = 0;
            self.next_leader();
            self.ask_leader();
        }
    }

    /// Over: who won, and every role told.
    fn end(&mut self, evil_won: bool) {
        let roles: Vec<String> = (0..self.seats.len())
            .map(|seat| format!("{} {}", self.name(seat), self.roles[seat].name()))
            .collect();
        self.log.push(Fact::all(format!(
            "{}赢了。身份：{}。",
            if evil_won { "坏人" } else { "好人" },
            roles.join("、")
        )));
        self.moments += 1;
        self.phases += 1;
        self.phase = Phase::Over { evil_won };
    }

    /// How it ended, as the group remembers it.
    pub fn outcome(&self) -> Option<String> {
        let Phase::Over { evil_won } = self.phase else {
            return None;
        };
        let evil = self.with(&Role::evil);
        let merlin = self.with(&|role| role == Role::Merlin);
        Some(format!(
            "{}赢了，坏人是{}，梅林是{}",
            if evil_won { "坏人" } else { "好人" },
            self.names(&evil),
            self.names(&merlin)
        ))
    }

    fn assassin(&self) -> Option<usize> {
        self.roles.iter().position(|role| *role == Role::Assassin)
    }

    /// Whom the assassin may name: the good (the evil know each other).
    fn targets(&self, assassin: usize) -> Vec<usize> {
        (0..self.seats.len())
            .filter(|seat| *seat != assassin && !self.roles[*seat].evil())
            .collect()
    }
}

/// The rules as she tells the table at the start.
fn told_rules(players: usize) -> String {
    let sizes: Vec<String> = team_sizes(players).iter().map(usize::to_string).collect();
    format!(
        "每人私聊收到自己的身份和该知道的人。每个任务由队长私聊一个个选出任务的人，然后大家私聊投赞成或反对，谁投了什么会公开；过半赞成就出发，否则换下一个人当队长，同一个任务连续 {REJECTIONS} 次没组成队坏人赢。出任务的人私聊出牌：好人只能出成功，坏人可以出成功或失败，有失败牌任务就失败{}。先赢三个任务的一方赢；好人先赢三个时，刺客还能刺杀一次，刺中梅林坏人赢。五个任务的人数：{}。",
        if players >= 7 {
            "（第四个任务要两张失败才失败）"
        } else {
            ""
        },
        sizes.join("、")
    )
}

impl Rules for Avalon {
    type State = Avalon;
    const KIND: &'static str = KIND;
    const RULES: &'static str = "In 阿瓦隆 (The Resistance: Avalon) each player was dealt a side and a role in secret. Merlin (good) knows the evil but must not be found out; Percival (good) sees Merlin and Morgana without knowing which is which; the evil know each other (except Oberon). For each of five quests a leader picks a team, everyone votes for or against it (votes are public), and the team, once more than half are for it, plays success or fail in secret: the good may only play success. Three quests one way end it; if the good win three, the assassin may name Merlin, and naming him rightly wins it for the evil. Five teams turned down in a row also win it for the evil.";

    fn seats(game: &Self) -> Vec<Seat> {
        game.seats.clone()
    }

    fn facts(game: &Self) -> Vec<Fact> {
        game.log.clone()
    }

    fn choices(game: &Self, seat: usize) -> Vec<String> {
        match &game.phase {
            Phase::Pick if seat == game.leader => (0..game.seats.len())
                .filter(|other| !game.team.contains(other))
                .map(|other| game.name(other).to_string())
                .collect(),
            Phase::Vote { votes } if !votes.iter().any(|(by, _)| *by == seat) => {
                vec![FOR.into(), AGAINST.into()]
            }
            Phase::Quest { cards }
                if game.team.contains(&seat) && !cards.iter().any(|(by, _)| *by == seat) =>
            {
                if game.roles[seat].evil() {
                    vec![SUCCESS.into(), FAIL.into()]
                } else {
                    vec![SUCCESS.into()]
                }
            }
            Phase::Assassinate if game.assassin() == Some(seat) => game
                .targets(seat)
                .into_iter()
                .map(|other| game.name(other).to_string())
                .collect(),
            _ => Vec::new(),
        }
    }

    fn take(game: &mut Self, seat: usize, index: usize, _: &mut Rolls) {
        if index >= Self::choices(game, seat).len() {
            return;
        }
        match game.phase.clone() {
            Phase::Pick => {
                let open: Vec<usize> = (0..game.seats.len())
                    .filter(|other| !game.team.contains(other))
                    .collect();
                game.team.push(open[index]);
                game.phases += 1;
                if game.team.len() >= game.size() {
                    let line = format!(
                        "队长{}选了：{}。大家私聊投赞成或反对。",
                        game.name(game.leader),
                        game.names(&game.team)
                    );
                    game.log.push(Fact::all(line));
                    game.phase = Phase::Vote { votes: Vec::new() };
                }
            }
            Phase::Vote { mut votes } => {
                votes.push((seat, index == 0));
                let all = votes.len() >= game.seats.len();
                game.phase = Phase::Vote { votes };
                if all {
                    game.count();
                }
            }
            Phase::Quest { mut cards } => {
                cards.push((seat, index == 0));
                let all = cards.len() >= game.team.len();
                game.phase = Phase::Quest { cards };
                if all {
                    game.reveal();
                }
            }
            Phase::Assassinate => {
                let target = game.targets(seat)[index];
                let hit = game.roles[target] == Role::Merlin;
                let line = format!(
                    "刺客{}刺杀了{}：{}。",
                    game.name(seat),
                    game.name(target),
                    if hit { "正是梅林" } else { "不是梅林" }
                );
                game.log.push(Fact::all(line));
                game.end(hit);
            }
            Phase::Over { .. } => {}
        }
    }

    fn over(game: &Self) -> bool {
        matches!(game.phase, Phase::Over { .. })
    }

    fn waiting(game: &Self) -> Waiting {
        match &game.phase {
            Phase::Pick => Waiting::Choose(vec![game.leader]),
            Phase::Vote { votes } => Waiting::Choose(
                (0..game.seats.len())
                    .filter(|seat| !votes.iter().any(|(by, _)| by == seat))
                    .collect(),
            ),
            Phase::Quest { cards } => Waiting::Choose(
                game.team
                    .iter()
                    .copied()
                    .filter(|seat| !cards.iter().any(|(by, _)| by == seat))
                    .collect(),
            ),
            Phase::Assassinate => Waiting::Choose(game.assassin().into_iter().collect()),
            Phase::Over { .. } => Waiting::Over,
        }
    }

    fn phase(game: &Self) -> u64 {
        game.phases
    }

    fn seconds(_: &Self) -> f64 {
        SECONDS
    }

    fn timed_out(game: &mut Self, _: &mut Rolls) {
        match game.phase.clone() {
            Phase::Pick => {
                let line = format!("队长{}没有选完，这次没组成队。", game.name(game.leader));
                game.log.push(Fact::all(line));
                game.turned_down();
            }
            Phase::Vote { .. } => game.count(),
            // Who did not play counts as success, and nobody is told who.
            Phase::Quest { cards } => {
                let mut cards = cards;
                for seat in game.team.clone() {
                    if !cards.iter().any(|(by, _)| *by == seat) {
                        cards.push((seat, true));
                    }
                }
                game.phase = Phase::Quest { cards };
                game.reveal();
            }
            Phase::Assassinate => {
                game.log.push(Fact::all("刺客没有出手。".to_string()));
                game.end(false);
            }
            Phase::Over { .. } => {}
        }
    }

    fn moments(game: &Self) -> usize {
        game.moments
    }

    /// Who is waited for is known to the table, but the assassin.
    fn public_wait(game: &Self) -> bool {
        !matches!(game.phase, Phase::Assassinate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::{Audience, narration_prompt, player_prompt, public_view, seat_view};

    /// Names none of which is part of another.
    const NAMES: [&str; 10] = [
        "阿明", "小红", "阿强", "小李", "大刘", "老王", "小周", "阿华", "小陈", "老张",
    ];

    fn seats(n: usize, hers: Option<usize>) -> Vec<Seat> {
        (0..n)
            .map(|seat| Seat {
                who: format!("telegram:{seat}"),
                name: NAMES[seat].to_string(),
                hers: hers == Some(seat),
            })
            .collect()
    }

    fn seat_of(game: &Avalon, role: Role) -> usize {
        game.roles.iter().position(|known| *known == role).unwrap()
    }

    fn pick(game: &mut Avalon, seat: usize, choice: &str, rolls: &mut Rolls) {
        let index = Avalon::choices(game, seat)
            .iter()
            .position(|known| known == choice)
            .unwrap_or_else(|| panic!("{choice} not among {:?}", Avalon::choices(game, seat)));
        Avalon::take(game, seat, index, rolls);
    }

    /// The leader picks `team`; everyone votes `for_it`.
    fn propose(game: &mut Avalon, team: &[usize], for_it: bool, rolls: &mut Rolls) {
        let leader = game.leader;
        for seat in team {
            let name = game.seats[*seat].name.clone();
            pick(game, leader, &name, rolls);
        }
        for seat in 0..game.seats.len() {
            pick(game, seat, if for_it { FOR } else { AGAINST }, rolls);
        }
    }

    fn known(game: &Avalon, seat: usize) -> String {
        seat_view::<Avalon>(game, seat).facts().join("\n")
    }

    #[test]
    fn boards_and_teams_by_players() {
        for players in FEWEST..=MOST {
            let roles = board(players);
            assert_eq!(roles.len(), players);
            let evil = roles.iter().filter(|role| role.evil()).count();
            assert_eq!(
                evil,
                match players {
                    5 | 6 => 2,
                    7..=9 => 3,
                    _ => 4,
                }
            );
            assert!(roles.contains(&Role::Merlin) && roles.contains(&Role::Assassin));
            assert!(team_sizes(players).iter().all(|size| *size < players));
        }
        assert_eq!(fails_needed(7, 3), 2);
        assert_eq!(fails_needed(6, 3), 1);
        assert_eq!(fails_needed(10, 2), 1);
    }

    #[test]
    fn who_knows_whom() {
        let mut rolls = Rolls::new(3, Vec::new());
        let game = Avalon::deal(seats(10, None), &mut rolls);
        let name = |role: Role| game.seats[seat_of(&game, role)].name.clone();
        let merlin = known(&game, seat_of(&game, Role::Merlin));
        for role in [Role::Morgana, Role::Assassin, Role::Oberon] {
            assert!(merlin.contains(&name(role)), "Merlin sees {role:?}");
        }
        let seen = merlin
            .lines()
            .find(|line| line.contains("你看到的坏人"))
            .unwrap();
        assert!(!seen.contains(&name(Role::Mordred)));
        let percival = known(&game, seat_of(&game, Role::Percival));
        let pair = percival
            .lines()
            .find(|line| line.contains("一个是梅林"))
            .unwrap();
        assert!(pair.contains(&name(Role::Merlin)) && pair.contains(&name(Role::Morgana)));
        let assassin = known(&game, seat_of(&game, Role::Assassin));
        let mates = assassin
            .lines()
            .find(|line| line.contains("坏人同伴"))
            .unwrap();
        assert!(mates.contains(&name(Role::Mordred)) && mates.contains(&name(Role::Morgana)));
        assert!(!mates.contains(&name(Role::Oberon)));
        let oberon = known(&game, seat_of(&game, Role::Oberon));
        assert!(oberon.contains("你不知道同伴是谁"));
        let loyal = known(&game, seat_of(&game, Role::Loyal));
        assert!(!loyal.contains("你看到的坏人") && !loyal.contains("同伴"));
    }

    #[test]
    fn the_good_play_only_success_and_one_fail_fails_it() {
        let mut rolls = Rolls::new(5, Vec::new());
        let mut game = Avalon::deal(seats(5, None), &mut rolls);
        let good = (0..5).find(|seat| !game.roles[*seat].evil()).unwrap();
        let evil = (0..5).find(|seat| game.roles[*seat].evil()).unwrap();
        propose(&mut game, &[good, evil], true, &mut rolls);
        assert_eq!(Avalon::choices(&game, good), vec![SUCCESS.to_string()]);
        assert_eq!(
            Avalon::choices(&game, evil),
            vec![SUCCESS.to_string(), FAIL.to_string()]
        );
        pick(&mut game, good, SUCCESS, &mut rolls);
        pick(&mut game, evil, FAIL, &mut rolls);
        assert_eq!(game.results, vec![false]);
        assert!(
            game.log
                .iter()
                .any(|fact| fact.text.contains("1 张失败，任务失败"))
        );
        assert!(matches!(game.phase, Phase::Pick));
    }

    #[test]
    fn the_fourth_quest_with_seven_needs_two_fails() {
        let mut rolls = Rolls::new(7, Vec::new());
        let mut game = Avalon::deal(seats(7, None), &mut rolls);
        game.results = vec![true, false, true];
        game.ask_leader();
        let evil = (0..7).find(|seat| game.roles[*seat].evil()).unwrap();
        let mut team = vec![evil];
        team.extend((0..7).filter(|seat| !game.roles[*seat].evil()).take(3));
        propose(&mut game, &team, true, &mut rolls);
        for seat in team {
            let card = if seat == evil { FAIL } else { SUCCESS };
            pick(&mut game, seat, card, &mut rolls);
        }
        assert_eq!(game.results, vec![true, false, true, true]);
        assert_eq!(game.phase, Phase::Assassinate);
    }

    #[test]
    fn five_teams_turned_down_win_it_for_the_evil() {
        let mut rolls = Rolls::new(11, Vec::new());
        let mut game = Avalon::deal(seats(6, None), &mut rolls);
        for _ in 0..REJECTIONS {
            let team: Vec<usize> = (0..2).collect();
            propose(&mut game, &team, false, &mut rolls);
        }
        assert_eq!(game.phase, Phase::Over { evil_won: true });
        assert!(
            game.log
                .iter()
                .any(|fact| fact.text.contains("连续 5 次没组成队"))
        );
    }

    #[test]
    fn a_vote_not_cast_is_not_for_it() {
        let mut rolls = Rolls::new(13, Vec::new());
        let mut game = Avalon::deal(seats(5, None), &mut rolls);
        let leader = game.leader;
        for seat in [0, 1] {
            let name = game.seats[seat].name.clone();
            pick(&mut game, leader, &name, &mut rolls);
        }
        for seat in 0..2 {
            pick(&mut game, seat, FOR, &mut rolls);
        }
        Avalon::timed_out(&mut game, &mut rolls);
        assert_eq!(game.rejections, 1);
        assert!(game.log.iter().any(|fact| fact.text.contains("没投")));
        assert_ne!(game.leader, leader);
    }

    #[test]
    fn the_assassin_naming_merlin_wins_it_for_the_evil() {
        for (aim, evil_won) in [(Role::Merlin, true), (Role::Percival, false)] {
            let mut rolls = Rolls::new(17, Vec::new());
            let mut game = Avalon::deal(seats(5, None), &mut rolls);
            game.results = vec![true, true, true];
            game.phase = Phase::Assassinate;
            let assassin = seat_of(&game, Role::Assassin);
            let target = game.seats[seat_of(&game, aim)].name.clone();
            assert!(Avalon::choices(&game, assassin).iter().all(|name| {
                game.seats
                    .iter()
                    .zip(&game.roles)
                    .any(|(seat, role)| seat.name == *name && !role.evil())
            }));
            pick(&mut game, assassin, &target, &mut rolls);
            assert_eq!(game.phase, Phase::Over { evil_won });
        }
    }

    /// Whatever the seed, the number of players and which seat is hers, at
    /// every step: nothing she is asked as her seat holds what was told to
    /// another seat alone (unless it was told to her too); nothing she is
    /// asked as the game's voice holds what was told to any seat alone; no
    /// role is told to all before the end.
    #[test]
    fn what_another_seat_was_told_never_reaches_her() {
        for seed in 0..400u64 {
            let mut rolls = Rolls::new(seed, Vec::new());
            let players = FEWEST + rolls.range(0..=(MOST - FEWEST) as u64) as usize;
            let hers = rolls.range(0..=(players as u64 - 1)) as usize;
            let mut game = Avalon::deal(seats(players, Some(hers)), &mut rolls);
            let mut steps = 0;
            while !Avalon::over(&game) && steps < 3000 {
                steps += 1;
                let view = seat_view::<Avalon>(&game, hers);
                let (s1, i1) = player_prompt("她", NAME, Avalon::RULES, &view);
                let (s2, i2) = narration_prompt("她", NAME, &public_view::<Avalon>(&game), &[]);
                let as_seat = [s1, i1].join("\n");
                let as_voice = [s2, i2].join("\n");
                for fact in &game.log {
                    let Audience::Seat(other) = fact.to else {
                        continue;
                    };
                    let quoted = serde_json::to_string(&fact.text).unwrap();
                    let quoted = &quoted[1..quoted.len() - 1];
                    if other != hers && !view.facts().contains(&fact.text) {
                        assert!(
                            !as_seat.contains(quoted),
                            "seed {seed}: {:?} reached her",
                            fact.text
                        );
                    }
                    assert!(
                        !as_voice.contains(quoted),
                        "seed {seed}: {:?} reached the voice",
                        fact.text
                    );
                }
                assert!(
                    !public_view::<Avalon>(&game)
                        .facts()
                        .join("\n")
                        .contains("身份："),
                    "seed {seed}: roles told before the end"
                );
                match Avalon::waiting(&game) {
                    Waiting::Choose(seats) if !seats.is_empty() && rolls.range(0..=9) != 0 => {
                        let seat = seats[rolls.range(0..=(seats.len() as u64 - 1)) as usize];
                        let open = Avalon::choices(&game, seat).len();
                        let index = rolls.range(0..=(open as u64 - 1)) as usize;
                        Avalon::take(&mut game, seat, index, &mut rolls);
                    }
                    Waiting::Choose(_) => Avalon::timed_out(&mut game, &mut rolls),
                    Waiting::Over => break,
                    _ => unreachable!("only choices"),
                }
            }
            assert!(Avalon::over(&game), "seed {seed}: never over");
            let end = public_view::<Avalon>(&game).facts().join("\n");
            for seat in 0..players {
                assert!(end.contains(&format!(
                    "{} {}",
                    game.seats[seat].name,
                    game.roles[seat].name()
                )));
            }
        }
    }
}
