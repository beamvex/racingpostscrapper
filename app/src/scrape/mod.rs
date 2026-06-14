mod browser;
mod full_results;
pub mod full_results_fetch;
pub mod full_results_loop;
pub mod full_results_write;
mod paths;
mod time_order;
mod urls;
pub mod urls_course;
pub mod urls_tsv;

pub use browser::connect_browser_and_spawn_handler;
pub use full_results::download_full_results_html;
pub use paths::out_base_dir_for_date;
pub use time_order::fetch_and_save_time_order_html;
pub use urls::write_url_files;
