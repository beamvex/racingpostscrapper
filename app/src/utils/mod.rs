mod date;
mod html_text;
mod rand;
mod svg;
mod text;

pub use date::current_utc_date_yyyy_mm_dd;
pub use html_text::strip_tags_and_collapse_ws;
pub use rand::pseudo_random_in_range;
pub use svg::remove_svg_blocks;
pub use text::sanitize_filename_component;
