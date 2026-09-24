//! Federation content publish, media upload, and author timeline helpers.
//!
//! Real submodules (not `include!`) so each file owns its imports and visibility.

mod ap_object;
mod kind;
mod media;
mod publish;
mod timeline;
mod types;

pub use kind::ContentKind;
pub(crate) use kind::REPOST_CONTENT_TYPE;
pub use media::classify_media_mime;
pub use publish::{create_note, list_published, publish_content, unpublish_content};
pub use types::{
    CreateNoteRequest, MediaUploadResponse, NoteAttachmentInput, PublishRequest, PublishResponse,
    PublishedAttachment, PublishedItem,
};

pub(crate) use ap_object::{
    FollowerRoute, StagedFanOut, deliver_to_local_followers, fan_out_to_followers, route_follower,
};
pub(crate) use timeline::preview_from_ap_object;
