# SPDX-License-Identifier: MIT
"""Firmware source inventory helper checked by rust/tests/landing_test.py."""


def source_files(args):
    chain = ['app_runtime/run_loop.rs', 'app_image.rs', 'reader_image_app.rs',
             'reader_image_work.rs', 'reader_image_operation.rs', 'reader_image_producer.rs',
             'reader_image_sources.rs', 'reader_image_index.rs',
             'reader_image_drive.rs', 'serial_console.rs', 'app_runtime/trace.rs',
             'reader_book_run.rs', 'reader_producer.rs', 'reader_image_metadata.rs',
             'reader_book_metadata.rs', 'reader_book_import.rs', 'reader_image_verification.rs',
             'reader_metadata.rs', 'reader_image_idle_save.rs', 'reader_checkpoint.rs',
             'reader_image_open.rs', 'reader_book.rs', 'reader_image_changed.rs',
             'reader_verification.rs']
    if args.pictures:
        chain += ['reader_image_picture.rs', 'reader_image_picture_work.rs',
                  'reader_image_picture_slot.rs', 'reader_image_picture_native.rs',
                  'reader_image_paint.rs', 'reader_image_action.rs', 'reader_image_exit.rs']
    if args.cold_geometry:
        chain += ['reader_image.rs', 'reader_image_boot.rs', 'reader_image_checkpoint.rs',
                  'reader_image_preferences.rs', 'reader_image_paint.rs', 'reader_image_exit.rs',
                  'home_startup.rs', 'home_startup/boot_resume.rs', 'reader_preferences.rs',
                  'retained_runtime.rs', 'reader_statistics_panel.rs']
    if args.home_import:
        chain += ['reader_image.rs', 'reader_image_boot.rs',
                  'reader_image_home_stats.rs', 'reader_cover_producer.rs',
                  'reader_image_hash.rs', '../salamander/native_io/crossink_files.rs',
                  '../salamander/native_io/posix_directory.rs', '../salamander/native_io/native/native_io.c',
                  '../salamander/native_io/native/posix_directory.c']
    if args.library:
        chain += ['reader_image.rs', 'reader_image_boot.rs', 'reader_image_session.rs',
                  'app_runtime/library.rs', 'rest_app.rs', '../salamander/native_io/library_folders.rs',
                  '../salamander/native_io/native/native_io.c']
    return chain
