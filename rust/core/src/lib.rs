//! Newt's shared, allocation-free core: books, durable records, updates and slot installer. MIT.
#![no_std]
#![forbid(unsafe_code)]

pub mod app_card;
pub mod app_store;
pub mod durable;
pub mod reader_action_profile;
pub mod preferences;
pub mod preferences_store;
pub mod progress;
pub mod progress_store;
pub mod reading_state_namespace;
pub mod refresh;
pub mod resume_picture;
pub mod sd_record;
pub mod wire;

pub mod clock_format;
pub mod ds3231;
pub mod frame;
pub mod time_zone;

pub mod library_name;
pub mod sd_resume;
pub mod selected_book;
pub mod sleep_intent;

pub mod stats_clock;
pub mod tracker;

pub mod reading_stats;
pub mod reading_stats_live;

pub mod library_catalog;
pub mod utf8;
pub mod zip_record_rules;

pub mod sleep_stats;
pub mod crossink_stats;
#[cfg(test)]
mod sleep_crossink_tests;

pub mod stats_graph_data;
pub mod book_days;

pub mod reading_stats_store;
pub mod book_days_store;
pub mod book_stats_import;
pub mod crossink_import;
#[cfg(test)]
mod statistics_store_tests;

pub mod external_progress_store;

pub mod reading_stats_service;
pub mod reading_stats_once;
pub mod stats_service_backend;

pub mod zip_window;

pub mod inflate;

pub mod zip_container;

pub mod css;

pub mod xml_input;

pub mod hyphenator;

pub mod font_layout;

pub mod image_probe;

pub mod xml_session;

pub mod package_parsers;

pub mod page_renderer;

pub mod image_renderer;
pub mod pngle;

pub mod app;
pub mod chapter_layout;

pub mod ft_font;

pub mod jpeg;

// Quick Start card persistence and policy.
pub mod quick_start;

pub mod admission;
pub mod canonical;
pub mod phased_metadata;

pub mod book_semantic_index;

pub mod page_cache;
pub mod page_envelope;
pub mod page_cache_access;

pub mod cache_names;

pub mod pause;
pub mod pause_store;

#[cfg(test)]
mod pause_tests;

pub mod producer_policy;

pub mod producer_visit;
pub mod reader_scheduler;

pub mod update_mode_request;

pub mod update_manifest;

pub mod update_release;

pub mod update_online;

pub mod update_card;

pub mod wifi_network_store;
pub mod update_seen;

pub mod network_clock;

pub mod update_confirm;

pub mod reader_source_hints;

pub mod save_notice;
pub mod reader_work_profile;
pub mod selection_authority;
pub mod cover;
