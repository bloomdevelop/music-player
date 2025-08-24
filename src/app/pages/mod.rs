use cosmic::prelude::*;

use super::{AppModel, Message, Page};

mod library;
mod now_playing;

pub fn page_view(app: &AppModel) -> Element<'_, Message> {
    let active_page = app
        .nav
        .data::<Page>(app.nav.active())
        .cloned()
        .unwrap_or(Page::Page1);

    match active_page {
        Page::Page1 => library::library_view(app),
        Page::Page2 => now_playing::now_playing_view(app)
    }
}
