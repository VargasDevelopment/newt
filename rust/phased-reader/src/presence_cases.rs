// Static independent fixture data: see vectors.json and NOTES.md. Not observations.
use super::*;
use crate::imported_revision::RevisionHasher;
use newt_core::preferences::{Orientation, TextSize};
struct Vector {
    settings: ViewSettings,
    spine: u32,
    digest: Digest,
    generation: u32,
    encoded: [u8; 99],
    sha: Digest,
    base: [u8; 69],
    map_encoded: [u8; 76],
    map_sha: Digest,
    map: [u8; 69],
}
const VECTORS: &[Vector] = &[
    Vector {
        settings: ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar: false,
            refresh_pages: 1,
            text_size: TextSize::Px30,
        },
        spine: 7,
        digest: [
            1, 18, 35, 52, 69, 86, 103, 120, 137, 154, 171, 188, 205, 222, 239, 0, 17, 34, 51, 68,
            85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 16,
        ],
        generation: 1754277706,
        encoded: [
            1, 18, 35, 52, 69, 86, 103, 120, 137, 154, 171, 188, 205, 222, 239, 0, 17, 34, 51, 68,
            85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 16, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 7, 0, 0, 0, 74, 39, 144, 104, 4, 0, 0, 0, 0, 0,
            1,
        ],
        sha: [
            4, 81, 80, 164, 252, 10, 144, 50, 152, 252, 207, 50, 33, 180, 233, 54, 88, 4, 219, 252,
            167, 163, 151, 137, 108, 250, 112, 145, 130, 218, 194, 13,
        ],
        base: *b"045150a4fc0a903298fccf3221b4e9365804dbfca7a397896cfa709182dac20d.fibp",
        map_encoded: [
            48, 52, 53, 49, 53, 48, 97, 52, 102, 99, 48, 97, 57, 48, 51, 50, 57, 56, 102, 99, 99,
            102, 51, 50, 50, 49, 98, 52, 101, 57, 51, 54, 53, 56, 48, 52, 100, 98, 102, 99, 97, 55,
            97, 51, 57, 55, 56, 57, 54, 99, 102, 97, 55, 48, 57, 49, 56, 50, 100, 97, 99, 50, 48,
            100, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            74, 89, 25, 236, 214, 200, 12, 102, 153, 247, 52, 80, 211, 235, 116, 173, 224, 212, 61,
            236, 137, 106, 180, 57, 215, 181, 218, 41, 154, 216, 92, 175,
        ],
        map: *b"4a5919ecd6c80c6699f73450d3eb74ade0d43dec896ab439d7b5da299ad85caf.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar: false,
            refresh_pages: 5,
            text_size: TextSize::Px30,
        },
        spine: 16909060,
        digest: [
            5, 22, 39, 56, 73, 90, 107, 124, 141, 158, 175, 192, 209, 226, 243, 4, 21, 38, 55, 72,
            89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20,
        ],
        generation: 1687167230,
        encoded: [
            5, 22, 39, 56, 73, 90, 107, 124, 141, 158, 175, 192, 209, 226, 243, 4, 21, 38, 55, 72,
            89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 254, 32, 144, 100, 4, 0, 0, 0, 0, 0,
            5,
        ],
        sha: [
            136, 198, 1, 222, 166, 177, 69, 205, 6, 17, 69, 213, 158, 29, 22, 218, 239, 187, 159,
            88, 152, 208, 60, 189, 252, 169, 205, 246, 153, 213, 133, 98,
        ],
        base: *b"88c601dea6b145cd061145d59e1d16daefbb9f5898d03cbdfca9cdf699d58562.fibp",
        map_encoded: [
            56, 56, 99, 54, 48, 49, 100, 101, 97, 54, 98, 49, 52, 53, 99, 100, 48, 54, 49, 49, 52,
            53, 100, 53, 57, 101, 49, 100, 49, 54, 100, 97, 101, 102, 98, 98, 57, 102, 53, 56, 57,
            56, 100, 48, 51, 99, 98, 100, 102, 99, 97, 57, 99, 100, 102, 54, 57, 57, 100, 53, 56,
            53, 54, 50, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            88, 151, 23, 228, 14, 15, 72, 24, 220, 90, 212, 141, 220, 21, 129, 14, 42, 108, 162,
            149, 155, 87, 200, 161, 246, 181, 64, 185, 15, 114, 106, 3,
        ],
        map: *b"589717e40e0f4818dc5ad48ddc15810e2a6ca2959b57c8a1f6b540b90f726a03.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar: false,
            refresh_pages: 10,
            text_size: TextSize::Px30,
        },
        spine: 7,
        digest: [
            10, 27, 44, 61, 78, 95, 112, 129, 146, 163, 180, 197, 214, 231, 248, 9, 26, 43, 60, 77,
            94, 111, 128, 145, 162, 179, 196, 213, 230, 247, 8, 25,
        ],
        generation: 1603279135,
        encoded: [
            10, 27, 44, 61, 78, 95, 112, 129, 146, 163, 180, 197, 214, 231, 248, 9, 26, 43, 60, 77,
            94, 111, 128, 145, 162, 179, 196, 213, 230, 247, 8, 25, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 7, 0, 0, 0, 31, 25, 144, 95, 4, 0, 0, 0, 0, 0,
            10,
        ],
        sha: [
            42, 113, 188, 105, 117, 26, 9, 95, 191, 246, 101, 216, 206, 172, 70, 197, 120, 24, 236,
            193, 111, 2, 125, 138, 137, 241, 237, 124, 120, 206, 224, 116,
        ],
        base: *b"2a71bc69751a095fbff665d8ceac46c57818ecc16f027d8a89f1ed7c78cee074.fibp",
        map_encoded: [
            50, 97, 55, 49, 98, 99, 54, 57, 55, 53, 49, 97, 48, 57, 53, 102, 98, 102, 102, 54, 54,
            53, 100, 56, 99, 101, 97, 99, 52, 54, 99, 53, 55, 56, 49, 56, 101, 99, 99, 49, 54, 102,
            48, 50, 55, 100, 56, 97, 56, 57, 102, 49, 101, 100, 55, 99, 55, 56, 99, 101, 101, 48,
            55, 52, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            58, 25, 104, 201, 193, 3, 147, 156, 197, 95, 87, 242, 4, 213, 124, 4, 149, 124, 190,
            43, 148, 79, 163, 216, 172, 121, 81, 73, 210, 24, 166, 37,
        ],
        map: *b"3a1968c9c103939cc55f57f204d57c04957cbe2b944fa3d8ac795149d218a625.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar: true,
            refresh_pages: 1,
            text_size: TextSize::Px30,
        },
        spine: 16909060,
        digest: [
            4, 21, 38, 55, 72, 89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 37, 54, 71,
            88, 105, 122, 139, 156, 173, 190, 207, 224, 241, 2, 19,
        ],
        generation: 1150136327,
        encoded: [
            4, 21, 38, 55, 72, 89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 37, 54, 71,
            88, 105, 122, 139, 156, 173, 190, 207, 224, 241, 2, 19, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 7, 176, 141, 68, 4, 0, 0, 0, 0, 1,
            1,
        ],
        sha: [
            5, 183, 141, 181, 35, 194, 174, 55, 125, 240, 151, 255, 240, 158, 18, 199, 253, 85,
            149, 76, 107, 230, 169, 247, 150, 108, 48, 86, 44, 107, 200, 90,
        ],
        base: *b"05b78db523c2ae377df097fff09e12c7fd55954c6be6a9f7966c30562c6bc85a.fibp",
        map_encoded: [
            48, 53, 98, 55, 56, 100, 98, 53, 50, 51, 99, 50, 97, 101, 51, 55, 55, 100, 102, 48, 57,
            55, 102, 102, 102, 48, 57, 101, 49, 50, 99, 55, 102, 100, 53, 53, 57, 53, 52, 99, 54,
            98, 101, 54, 97, 57, 102, 55, 57, 54, 54, 99, 51, 48, 53, 54, 50, 99, 54, 98, 99, 56,
            53, 97, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            108, 197, 231, 115, 127, 169, 128, 101, 197, 21, 25, 59, 93, 137, 97, 69, 138, 0, 40,
            201, 253, 186, 222, 215, 237, 41, 135, 62, 0, 100, 141, 252,
        ],
        map: *b"6cc5e7737fa98065c515193b5d8961458a0028c9fdbaded7ed29873e00648dfc.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar: true,
            refresh_pages: 5,
            text_size: TextSize::Px30,
        },
        spine: 0,
        digest: [
            8, 25, 42, 59, 76, 93, 110, 127, 144, 161, 178, 195, 212, 229, 246, 7, 24, 41, 58, 75,
            92, 109, 126, 143, 160, 177, 194, 211, 228, 245, 6, 23,
        ],
        generation: 1083025851,
        encoded: [
            8, 25, 42, 59, 76, 93, 110, 127, 144, 161, 178, 195, 212, 229, 246, 7, 24, 41, 58, 75,
            92, 109, 126, 143, 160, 177, 194, 211, 228, 245, 6, 23, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 0, 0, 0, 0, 187, 169, 141, 64, 4, 0, 0, 0, 0, 1,
            5,
        ],
        sha: [
            28, 178, 250, 65, 140, 112, 146, 55, 29, 183, 152, 149, 240, 215, 128, 204, 159, 115,
            228, 158, 235, 137, 108, 76, 177, 151, 33, 171, 54, 166, 110, 239,
        ],
        base: *b"1cb2fa418c7092371db79895f0d780cc9f73e49eeb896c4cb19721ab36a66eef.fibp",
        map_encoded: [
            49, 99, 98, 50, 102, 97, 52, 49, 56, 99, 55, 48, 57, 50, 51, 55, 49, 100, 98, 55, 57,
            56, 57, 53, 102, 48, 100, 55, 56, 48, 99, 99, 57, 102, 55, 51, 101, 52, 57, 101, 101,
            98, 56, 57, 54, 99, 52, 99, 98, 49, 57, 55, 50, 49, 97, 98, 51, 54, 97, 54, 54, 101,
            101, 102, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            50, 179, 118, 18, 247, 125, 1, 12, 91, 227, 61, 239, 75, 196, 189, 228, 7, 15, 91, 250,
            139, 122, 251, 151, 31, 24, 42, 143, 84, 18, 231, 209,
        ],
        map: *b"32b37612f77d010c5be33def4bc4bde4070f5bfa8b7afb971f182a8f5412e7d1.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::Portrait,
            slim_progress_bar: true,
            refresh_pages: 10,
            text_size: TextSize::Px30,
        },
        spine: 16909060,
        digest: [
            13, 30, 47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 46, 63,
            80, 97, 114, 131, 148, 165, 182, 199, 216, 233, 250, 11, 28,
        ],
        generation: 1032692994,
        encoded: [
            13, 30, 47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 46, 63,
            80, 97, 114, 131, 148, 165, 182, 199, 216, 233, 250, 11, 28, 206, 43, 218, 116, 11, 0,
            49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 2, 165, 141, 61, 4, 0, 0, 0, 0, 1,
            10,
        ],
        sha: [
            32, 146, 28, 67, 154, 118, 2, 43, 136, 24, 152, 89, 3, 239, 104, 249, 247, 81, 75, 153,
            116, 30, 109, 56, 119, 205, 227, 9, 14, 69, 94, 95,
        ],
        base: *b"20921c439a76022b8818985903ef68f9f7514b99741e6d3877cde3090e455e5f.fibp",
        map_encoded: [
            50, 48, 57, 50, 49, 99, 52, 51, 57, 97, 55, 54, 48, 50, 50, 98, 56, 56, 49, 56, 57, 56,
            53, 57, 48, 51, 101, 102, 54, 56, 102, 57, 102, 55, 53, 49, 52, 98, 57, 57, 55, 52, 49,
            101, 54, 100, 51, 56, 55, 55, 99, 100, 101, 51, 48, 57, 48, 101, 52, 53, 53, 101, 53,
            102, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            114, 176, 73, 223, 201, 176, 176, 193, 223, 50, 168, 8, 163, 52, 252, 22, 111, 183,
            106, 38, 40, 37, 111, 57, 84, 188, 57, 123, 201, 89, 81, 85,
        ],
        map: *b"72b049dfc9b0b0c1df32a808a334fc166fb76a2628256f3954bc397bc9595155.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: false,
            refresh_pages: 1,
            text_size: TextSize::Px30,
        },
        spine: 7,
        digest: [
            10, 27, 44, 61, 78, 95, 112, 129, 146, 163, 180, 197, 214, 231, 248, 9, 26, 43, 60, 77,
            94, 111, 128, 145, 162, 179, 196, 213, 230, 247, 8, 25,
        ],
        generation: 1938441498,
        encoded: [
            10, 27, 44, 61, 78, 95, 112, 129, 146, 163, 180, 197, 214, 231, 248, 9, 26, 43, 60, 77,
            94, 111, 128, 145, 162, 179, 196, 213, 230, 247, 8, 25, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 7, 0, 0, 0, 26, 69, 138, 115, 4, 0, 0, 0, 1, 0,
            1,
        ],
        sha: [
            55, 27, 89, 130, 238, 131, 33, 55, 39, 33, 113, 96, 102, 152, 35, 92, 80, 84, 230, 187,
            145, 171, 22, 86, 81, 27, 198, 199, 121, 121, 32, 104,
        ],
        base: *b"371b5982ee832137272171606698235c5054e6bb91ab1656511bc6c779792068.fibp",
        map_encoded: [
            51, 55, 49, 98, 53, 57, 56, 50, 101, 101, 56, 51, 50, 49, 51, 55, 50, 55, 50, 49, 55,
            49, 54, 48, 54, 54, 57, 56, 50, 51, 53, 99, 53, 48, 53, 52, 101, 54, 98, 98, 57, 49,
            97, 98, 49, 54, 53, 54, 53, 49, 49, 98, 99, 54, 99, 55, 55, 57, 55, 57, 50, 48, 54, 56,
            255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            207, 64, 139, 32, 17, 128, 134, 243, 167, 137, 164, 144, 34, 157, 43, 126, 211, 89, 48,
            159, 213, 93, 114, 255, 180, 84, 97, 124, 156, 75, 105, 9,
        ],
        map: *b"cf408b20118086f3a789a490229d2b7ed359309fd55d72ffb454617c9c4b6909.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: false,
            refresh_pages: 5,
            text_size: TextSize::Px30,
        },
        spine: 16909060,
        digest: [
            14, 31, 48, 65, 82, 99, 116, 133, 150, 167, 184, 201, 218, 235, 252, 13, 30, 47, 64,
            81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29,
        ],
        generation: 1871331022,
        encoded: [
            14, 31, 48, 65, 82, 99, 116, 133, 150, 167, 184, 201, 218, 235, 252, 13, 30, 47, 64,
            81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 206, 43, 218, 116, 11, 0,
            49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 206, 62, 138, 111, 4, 0, 0, 0, 1, 0,
            5,
        ],
        sha: [
            61, 38, 130, 48, 19, 176, 37, 98, 68, 169, 146, 80, 61, 128, 72, 121, 124, 142, 170,
            137, 54, 29, 17, 4, 82, 66, 9, 116, 220, 145, 214, 123,
        ],
        base: *b"3d26823013b0256244a992503d8048797c8eaa89361d110452420974dc91d67b.fibp",
        map_encoded: [
            51, 100, 50, 54, 56, 50, 51, 48, 49, 51, 98, 48, 50, 53, 54, 50, 52, 52, 97, 57, 57,
            50, 53, 48, 51, 100, 56, 48, 52, 56, 55, 57, 55, 99, 56, 101, 97, 97, 56, 57, 51, 54,
            49, 100, 49, 49, 48, 52, 53, 50, 52, 50, 48, 57, 55, 52, 100, 99, 57, 49, 100, 54, 55,
            98, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            226, 42, 147, 20, 47, 110, 147, 16, 143, 31, 164, 112, 187, 209, 200, 190, 31, 222,
            156, 178, 250, 7, 57, 150, 1, 97, 126, 134, 220, 171, 20, 74,
        ],
        map: *b"e22a93142f6e93108f1fa470bbd1c8be1fde9cb2fa07399601617e86dcab144a.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: false,
            refresh_pages: 10,
            text_size: TextSize::Px30,
        },
        spine: 7,
        digest: [
            19, 36, 53, 70, 87, 104, 121, 138, 155, 172, 189, 206, 223, 240, 1, 18, 35, 52, 69, 86,
            103, 120, 137, 154, 171, 188, 205, 222, 239, 0, 17, 34,
        ],
        generation: 1787442927,
        encoded: [
            19, 36, 53, 70, 87, 104, 121, 138, 155, 172, 189, 206, 223, 240, 1, 18, 35, 52, 69, 86,
            103, 120, 137, 154, 171, 188, 205, 222, 239, 0, 17, 34, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 7, 0, 0, 0, 239, 54, 138, 106, 4, 0, 0, 0, 1, 0,
            10,
        ],
        sha: [
            85, 165, 235, 184, 184, 180, 173, 136, 163, 146, 249, 137, 255, 85, 99, 47, 76, 80, 99,
            135, 135, 133, 57, 44, 237, 207, 26, 163, 7, 140, 236, 209,
        ],
        base: *b"55a5ebb8b8b4ad88a392f989ff55632f4c5063878785392cedcf1aa3078cecd1.fibp",
        map_encoded: [
            53, 53, 97, 53, 101, 98, 98, 56, 98, 56, 98, 52, 97, 100, 56, 56, 97, 51, 57, 50, 102,
            57, 56, 57, 102, 102, 53, 53, 54, 51, 50, 102, 52, 99, 53, 48, 54, 51, 56, 55, 56, 55,
            56, 53, 51, 57, 50, 99, 101, 100, 99, 102, 49, 97, 97, 51, 48, 55, 56, 99, 101, 99,
            100, 49, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            216, 45, 59, 251, 32, 152, 54, 184, 159, 144, 112, 235, 184, 58, 210, 207, 7, 133, 90,
            109, 99, 194, 37, 88, 177, 111, 21, 21, 14, 22, 214, 253,
        ],
        map: *b"d82d3bfb209836b89f9070ebb83ad2cf07855a6d63c22558b16f15150e16d6fd.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: true,
            refresh_pages: 1,
            text_size: TextSize::Px30,
        },
        spine: 16909060,
        digest: [
            13, 30, 47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 46, 63,
            80, 97, 114, 131, 148, 165, 182, 199, 216, 233, 250, 11, 28,
        ],
        generation: 1334300119,
        encoded: [
            13, 30, 47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 46, 63,
            80, 97, 114, 131, 148, 165, 182, 199, 216, 233, 250, 11, 28, 206, 43, 218, 116, 11, 0,
            49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 215, 205, 135, 79, 4, 0, 0, 0, 1, 1,
            1,
        ],
        sha: [
            252, 151, 96, 251, 241, 109, 108, 47, 111, 142, 75, 234, 47, 49, 108, 0, 234, 142, 82,
            136, 216, 8, 17, 46, 241, 113, 184, 0, 99, 156, 138, 237,
        ],
        base: *b"fc9760fbf16d6c2f6f8e4bea2f316c00ea8e5288d808112ef171b800639c8aed.fibp",
        map_encoded: [
            102, 99, 57, 55, 54, 48, 102, 98, 102, 49, 54, 100, 54, 99, 50, 102, 54, 102, 56, 101,
            52, 98, 101, 97, 50, 102, 51, 49, 54, 99, 48, 48, 101, 97, 56, 101, 53, 50, 56, 56,
            100, 56, 48, 56, 49, 49, 50, 101, 102, 49, 55, 49, 98, 56, 48, 48, 54, 51, 57, 99, 56,
            97, 101, 100, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            99, 127, 221, 225, 13, 222, 120, 234, 63, 40, 57, 249, 218, 1, 33, 179, 234, 77, 44,
            43, 96, 84, 119, 155, 208, 105, 62, 12, 234, 134, 134, 99,
        ],
        map: *b"637fdde10dde78ea3f2839f9da0121b3ea4d2c2b6054779bd0693e0cea868663.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: true,
            refresh_pages: 5,
            text_size: TextSize::Px30,
        },
        spine: 0,
        digest: [
            17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 16, 33, 50, 67,
            84, 101, 118, 135, 152, 169, 186, 203, 220, 237, 254, 15, 32,
        ],
        generation: 1267189643,
        encoded: [
            17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 16, 33, 50, 67,
            84, 101, 118, 135, 152, 169, 186, 203, 220, 237, 254, 15, 32, 206, 43, 218, 116, 11, 0,
            49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 0, 0, 0, 0, 139, 199, 135, 75, 4, 0, 0, 0, 1, 1,
            5,
        ],
        sha: [
            56, 25, 180, 198, 3, 2, 172, 179, 117, 213, 136, 23, 236, 151, 90, 239, 134, 223, 179,
            174, 93, 238, 0, 114, 31, 246, 52, 191, 196, 91, 109, 114,
        ],
        base: *b"3819b4c60302acb375d58817ec975aef86dfb3ae5dee00721ff634bfc45b6d72.fibp",
        map_encoded: [
            51, 56, 49, 57, 98, 52, 99, 54, 48, 51, 48, 50, 97, 99, 98, 51, 55, 53, 100, 53, 56,
            56, 49, 55, 101, 99, 57, 55, 53, 97, 101, 102, 56, 54, 100, 102, 98, 51, 97, 101, 53,
            100, 101, 101, 48, 48, 55, 50, 49, 102, 102, 54, 51, 52, 98, 102, 99, 52, 53, 98, 54,
            100, 55, 50, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            40, 101, 236, 165, 208, 223, 217, 13, 120, 222, 183, 226, 200, 13, 84, 33, 79, 208, 72,
            216, 52, 223, 187, 149, 109, 102, 144, 23, 232, 169, 217, 29,
        ],
        map: *b"2865eca5d0dfd90d78deb7e2c80d54214fd048d834dfbb956d669017e8a9d91d.fibp",
    },
    Vector {
        settings: ViewSettings {
            orientation: Orientation::LandscapeCcw,
            slim_progress_bar: true,
            refresh_pages: 10,
            text_size: TextSize::Px30,
        },
        spine: 16909060,
        digest: [
            22, 39, 56, 73, 90, 107, 124, 141, 158, 175, 192, 209, 226, 243, 4, 21, 38, 55, 72, 89,
            106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 37,
        ],
        generation: 1216856786,
        encoded: [
            22, 39, 56, 73, 90, 107, 124, 141, 158, 175, 192, 209, 226, 243, 4, 21, 38, 55, 72, 89,
            106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 37, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 210, 194, 135, 72, 4, 0, 0, 0, 1, 1,
            10,
        ],
        sha: [
            60, 35, 233, 230, 241, 45, 157, 70, 14, 72, 72, 69, 204, 151, 160, 3, 71, 119, 140, 73,
            38, 25, 254, 8, 252, 249, 183, 29, 187, 39, 29, 172,
        ],
        base: *b"3c23e9e6f12d9d460e484845cc97a00347778c492619fe08fcf9b71dbb271dac.fibp",
        map_encoded: [
            51, 99, 50, 51, 101, 57, 101, 54, 102, 49, 50, 100, 57, 100, 52, 54, 48, 101, 52, 56,
            52, 56, 52, 53, 99, 99, 57, 55, 97, 48, 48, 51, 52, 55, 55, 55, 56, 99, 52, 57, 50, 54,
            49, 57, 102, 101, 48, 56, 102, 99, 102, 57, 98, 55, 49, 100, 98, 98, 50, 55, 49, 100,
            97, 99, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            112, 41, 42, 33, 131, 4, 130, 90, 32, 179, 148, 96, 224, 151, 54, 186, 208, 240, 120,
            245, 102, 183, 84, 211, 192, 21, 250, 199, 217, 16, 101, 183,
        ],
        map: *b"70292a218304825a20b39460e09736bad0f078f566b754d3c015fac7d91065b7.fibp",
    },
];
// Scripted SHA transcript, not a hash implementation. Checks exact input bytes;
// returns independent SHA expected bytes. Root must also run with actual SHA.
struct Hash<'a> {
    v: &'a Vector,
    calls: usize,
    fail: usize,
}
impl RevisionHasher for Hash<'_> {
    fn begin(&mut self) -> bool {
        panic!("presence must use one-shot SHA")
    }
    fn update(&mut self, _: &[u8]) -> bool {
        panic!("stream SHA in presence")
    }
    fn finish(&mut self, _: &mut Digest) -> bool {
        panic!("stream SHA in presence")
    }
    fn hash(&mut self, input: &[u8], output: &mut Digest) -> bool {
        self.calls += 1;
        if self.calls == 1 {
            assert_eq!(input, self.v.encoded);
            *output = self.v.sha;
        } else {
            assert_eq!(self.calls, 2);
            assert_eq!(input, self.v.map_encoded);
            *output = self.v.map_sha;
        }
        self.calls != self.fail
    }
}
#[test]
fn independent_identity_presence_and_failures() {
    for orientation in [Orientation::Portrait, Orientation::LandscapeCcw] {
        let settings = ViewSettings { orientation, ..ViewSettings::default() };
        let generations = BookLanguage::ALL.map(|language| identity_generation(settings, language));
        for (index, generation) in generations.iter().enumerate() {
            assert!(!generations[..index].contains(generation));
        }
    }
    // Each text size lays out its own pages, so it names its own caches. The
    // pinned VECTORS below keep the 30 px names unchanged.
    let mut sized = [0; 2 * TextSize::ALL.len()];
    for (index, slot) in sized.iter_mut().enumerate() {
        let orientation = if index % 2 == 0 { Orientation::Portrait } else { Orientation::LandscapeCcw };
        let text_size = TextSize::ALL[index / 2];
        *slot = identity_generation(ViewSettings { orientation, text_size, ..ViewSettings::default() }, BookLanguage::default());
    }
    for (index, generation) in sized.iter().enumerate() {
        assert!(!sized[..index].contains(generation), "text size {index} shares a cache generation");
    }
    for v in VECTORS {
        let mut h = Hash {
            v,
            calls: 0,
            fail: 0,
        };
        assert_eq!(
            identity(v.settings, &v.digest, v.spine, &mut h, BookLanguage::default()),
            Ok((v.base, v.generation))
        );
        // Clockwise hashes the counterclockwise bytes, so it reuses those pages.
        if v.settings.orientation == Orientation::LandscapeCcw {
            let turned = ViewSettings { orientation: Orientation::LandscapeCw, ..v.settings };
            let mut h = Hash { v, calls: 0, fail: 0 };
            assert_eq!(identity(turned, &v.digest, v.spine, &mut h, BookLanguage::default()),
                Ok((v.base, v.generation)));
        }
        // Existing base covers empty/invalid files: contents are never consulted.
        for (base, map, fail, answer, hc, pc) in [
            (true, false, 0, true, 1, 1),
            (true, true, 2, true, 1, 1),
            (false, true, 0, true, 2, 2),
            (false, false, 0, false, 2, 2),
            (false, true, 1, false, 1, 0),
            (false, true, 2, false, 2, 1),
        ] {
            let mut h = Hash { v, calls: 0, fail };
            let mut calls = 0;
            let result = cached_for_language(v.settings, &v.digest, v.spine, &mut h, BookLanguage::default(), &mut |name| {
                calls += 1;
                if calls == 1 {
                    assert_eq!(name, v.base);
                    base
                } else {
                    assert_eq!(calls, 2);
                    assert_eq!(name, v.map);
                    map
                }
            });
            assert_eq!((result, h.calls, calls), (answer, hc, pc));
        }
    }
}

// Root calls this with real already-allocated reader/resource loans. This is
// an executable scenario body, not a fabricated Reader/Parser implementation.
#[allow(dead_code)]
pub fn actual_reader_cases<B: ParserBackend + ?Sized>(
    r: &mut OperationReader<'_, '_, '_, '_, '_, '_, B>,
    memory: &mut WriterMemory<'_>,
) {
    struct Store {
        calls: usize,
        opening: bool,
        name: [u8; 69],
    }
    impl CachePresence for Store {
        fn present(&mut self, name: &[u8], opening: bool) -> bool {
            self.calls += 1;
            self.opening = opening;
            assert_eq!(name, self.name);
            true
        }
    }
    impl CacheStorage for Store {
        fn remove(&mut self, _: &[u8]) -> bool {
            panic!("remove")
        }
        fn file_size(&mut self, _: &[u8]) -> i64 {
            panic!("size is not exists")
        }
        fn read_at(&mut self, _: &[u8], _: u32, _: &mut [u8]) -> i32 {
            panic!("footer read")
        }
        fn begin_write(&mut self, _: &[u8]) -> bool {
            panic!("write")
        }
        fn write(&mut self, _: &[u8]) -> bool {
            panic!("write")
        }
        fn end_write(&mut self) -> bool {
            panic!("write")
        }
    }
    let old_layout = r.reader.page_layout;
    let old_page = r.reader.has_page;
    let old_opened = r.reader.opened;
    r.reader.opened = false;
    for v in VECTORS
        .iter()
        .filter(|v| v.settings.slim_progress_bar && v.settings.refresh_pages == 5)
    {
        r.reader.page_layout = v.settings.page_layout();
        for page in [false, true] {
            r.reader.has_page = page;
            let mut store = Store {
                calls: 0,
                opening: false,
                name: v.base,
            };
            let mut hash = Hash {
                v,
                calls: 0,
                fail: 0,
            };
            let plan = r.reader.plan.take();
            {
                let mut cache = OperationCache {
                    store: &mut store,
                    memory,
                    hash: &mut hash,
                    settings: v.settings,
                };
                assert!(!session::Cache::stored(&mut cache, &v.digest, v.spine, r));
            }
            assert_eq!((hash.calls, store.calls), (0, 0));
            r.reader.plan = plan;
            assert!(r.reader.plan.is_some(), "root must supply actual plan");
            {
                let mut cache = OperationCache {
                    store: &mut store,
                    memory,
                    hash: &mut hash,
                    settings: v.settings,
                };
                assert!(session::Cache::stored(&mut cache, &v.digest, v.spine, r));
            }
            assert_eq!((hash.calls, store.calls, store.opening), (1, 1, !page));
            for bad in [
                ViewSettings {
                    orientation: if r.reader.page_layout.landscape {
                        Orientation::Portrait
                    } else {
                        Orientation::LandscapeCcw
                    },
                    ..v.settings
                },
                ViewSettings {
                    slim_progress_bar: false,
                    ..v.settings
                },
                ViewSettings {
                    refresh_pages: 10,
                    ..v.settings
                },
            ] {
                let mut cache = OperationCache {
                    store: &mut store,
                    memory,
                    hash: &mut hash,
                    settings: bad,
                };
                assert!(!session::Cache::stored(&mut cache, &v.digest, v.spine, r));
            }
            assert_eq!((hash.calls, store.calls), (1, 1));
        }
    }
    r.reader.page_layout = old_layout;
    r.reader.has_page = old_page;
    r.reader.opened = old_opened;
}

// Root supplies the existing disjoint native SHA loan, not the transcript SHA.
#[allow(dead_code)]
pub fn actual_sha_cases(hash: &mut dyn crate::hash_provider::CacheHash) {
    for v in VECTORS {
        assert_eq!(
            identity(v.settings, &v.digest, v.spine, hash, BookLanguage::default()),
            Ok((v.base, v.generation))
        );
        let mut calls = 0;
        assert!(cached_for_language(v.settings, &v.digest, v.spine, hash, BookLanguage::default(), &mut |name| {
            calls += 1;
            if calls == 1 {
                assert_eq!(name, v.base);
                false
            } else {
                assert_eq!(name, v.map);
                true
            }
        }));
        assert_eq!(calls, 2);
    }
}

#[test]
fn legacy_raw_exists_counts_empty_and_invalid_files() {
    // Counted flat names with raw content, not a valid PageCache record.
    // The same existence operation handles empty and invalid existing records.
    struct Files<'a> {
        names: [&'a [u8]; 2],
        bytes: [&'a [u8]; 2],
        probes: usize,
    }
    let mut files = Files {
        names: [b"empty.fibp", b"invalid.fibp"],
        bytes: [b"", b"bad footer"],
        probes: 0,
    };
    let exists = |files: &mut Files<'_>, name: &[u8]| {
        files.probes += 1;
        files.names.iter().any(|n| *n == name)
    };
    for opening in [true, false] {
        let mut store = PresenceStore {
            store: &mut files,
            exists,
        };
        assert!(store.present(b"empty.fibp", opening));
        assert!(store.present(b"invalid.fibp", opening));
        assert!(!store.present(b"absent.fibp", opening));
    }
    assert_eq!(files.probes, 6);
    assert_eq!(files.bytes, [b"" as &[u8], b"bad footer"]);
}
