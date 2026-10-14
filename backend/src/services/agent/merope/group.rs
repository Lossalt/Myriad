//! What the chat-app services (`channel_group`, `channel_work`,
//! `channel_pairing`) use of her: whether she joins a group's talk, when she
//! sees and answers, what she makes of pictures and stickers, and strangers.
//! Her modules stay inside `services::agent`; anything a channel needs is
//! named here first.

pub mod asking {
    pub(crate) use crate::services::agent::merope::draft::ask as judge;
}

pub mod bits {
    pub(crate) use crate::services::agent::merope::bits::{
        go_over_stretch, in_group, picture_again,
    };
}

pub mod doing {
    pub(crate) use crate::services::agent::merope::doing::{
        current, now_text, recalled, would_tell,
    };
}

pub mod heard {
    pub(crate) use crate::services::agent::merope::heard::{SOURCE, take_in};
}

pub mod jargon {
    pub(crate) use crate::services::agent::merope::jargon::{heard, nominate, section_for};
}

pub mod memes {
    pub(crate) use crate::services::agent::merope::memes::{may_look_up, section_for};
}

pub mod senses {
    pub(crate) use crate::services::agent::merope::curiosity::look_up;
    pub(crate) use crate::services::agent::merope::senses::define;
}

pub mod others {
    pub(crate) use crate::services::agent::merope::others::{spoke_up, spoke_up_lately};
}

pub mod seeing {
    pub(crate) use crate::services::agent::merope::seeing::{known, look};
}

pub mod sharing {
    pub(crate) use crate::services::agent::merope::sharing::{Offered, choose, come_back};
}

pub mod stickers {
    pub(crate) use crate::services::agent::merope::stickers::{
        keep_theirs, picture, resolve, sent,
    };
}

pub mod strangers {
    pub(crate) use crate::services::agent::merope::strangers::{
        Stranger, adopt, enqueue_after, reply,
    };
}

/// Her memory work queue, driven by a recorded scene.
#[cfg(test)]
pub mod memory_jobs {
    pub(crate) use crate::services::agent::merope::memory_jobs::tick;
}

pub mod timing {
    pub(crate) use crate::services::agent::merope::timing::{
        asleep_now, typing, until_read, where_she_is,
    };
}
