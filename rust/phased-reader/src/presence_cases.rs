// Static independent fixture data: see vectors.json and NOTES.md. Not observations.
use super::*;
use crate::imported_revision::RevisionHasher;
use newt_core::preferences::{Orientation, TextSize};
struct Vector {
    settings: ViewSettings,
    spine: u32,
    digest: Digest,
    generation: u32,
    encoded: [u8; 97],
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
        generation: 217317991,
        encoded: [
            1, 18, 35, 52, 69, 86, 103, 120, 137, 154, 171, 188, 205, 222, 239, 0, 17,
            34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 16, 206,
            43, 218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197,
            147, 131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205,
            111, 91, 131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117,
            240, 33, 7, 0, 0, 0, 103, 2, 244, 12, 6, 0, 0, 0, 0,
        ],
        sha: [
            128, 182, 77, 156, 226, 81, 47, 209, 167, 217, 236, 93, 160, 130, 198, 55,
            69, 193, 34, 174, 35, 87, 111, 58, 195, 158, 15, 237, 128, 192, 31, 107,
        ],
        base: *b"80b64d9ce2512fd1a7d9ec5da082c63745c122ae23576f3ac39e0fed80c01f6b.fibp",
        map_encoded: [
            56, 48, 98, 54, 52, 100, 57, 99, 101, 50, 53, 49, 50, 102, 100, 49, 97, 55,
            100, 57, 101, 99, 53, 100, 97, 48, 56, 50, 99, 54, 51, 55, 52, 53, 99, 49,
            50, 50, 97, 101, 50, 51, 53, 55, 54, 102, 51, 97, 99, 51, 57, 101, 48, 102,
            101, 100, 56, 48, 99, 48, 49, 102, 54, 98, 255, 255, 255, 255, 0, 0, 0, 0,
            0, 0, 0, 0,
        ],
        map_sha: [
            242, 128, 211, 163, 28, 253, 94, 202, 83, 118, 211, 210, 69, 55, 247, 68,
            116, 198, 236, 119, 169, 3, 190, 87, 188, 107, 237, 170, 121, 227, 73, 56,
        ],
        map: *b"f280d3a31cfd5eca5376d3d24537f74474c6ec77a903be57bc6bedaa79e34938.fibp",
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
        generation: 217317991,
        encoded: [
            5, 22, 39, 56, 73, 90, 107, 124, 141, 158, 175, 192, 209, 226, 243, 4, 21,
            38, 55, 72, 89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 206, 43,
            218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147,
            131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205, 111, 91,
            131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117, 240, 33,
            4, 3, 2, 1, 103, 2, 244, 12, 6, 0, 0, 0, 0,
        ],
        sha: [
            167, 96, 2, 95, 56, 231, 25, 22, 139, 206, 232, 160, 30, 23, 11, 133, 26,
            144, 133, 132, 62, 107, 88, 218, 185, 204, 153, 47, 81, 36, 113, 171,
        ],
        base: *b"a760025f38e719168bcee8a01e170b851a9085843e6b58dab9cc992f512471ab.fibp",
        map_encoded: [
            97, 55, 54, 48, 48, 50, 53, 102, 51, 56, 101, 55, 49, 57, 49, 54, 56, 98,
            99, 101, 101, 56, 97, 48, 49, 101, 49, 55, 48, 98, 56, 53, 49, 97, 57, 48,
            56, 53, 56, 52, 51, 101, 54, 98, 53, 56, 100, 97, 98, 57, 99, 99, 57, 57,
            50, 102, 53, 49, 50, 52, 55, 49, 97, 98, 255, 255, 255, 255, 0, 0, 0, 0, 0,
            0, 0, 0,
        ],
        map_sha: [
            209, 241, 10, 135, 228, 71, 82, 86, 80, 216, 58, 248, 150, 201, 12, 168, 79,
            59, 107, 198, 70, 108, 12, 186, 51, 141, 7, 137, 93, 239, 239, 65,
        ],
        map: *b"d1f10a87e447525650d83af896c90ca84f3b6bc6466c0cba338d07895defef41.fibp",
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
        generation: 217317991,
        encoded: [
            10, 27, 44, 61, 78, 95, 112, 129, 146, 163, 180, 197, 214, 231, 248, 9, 26,
            43, 60, 77, 94, 111, 128, 145, 162, 179, 196, 213, 230, 247, 8, 25, 206, 43,
            218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147,
            131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205, 111, 91,
            131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117, 240, 33,
            7, 0, 0, 0, 103, 2, 244, 12, 6, 0, 0, 0, 0,
        ],
        sha: [
            200, 69, 232, 206, 69, 122, 222, 249, 47, 21, 24, 37, 248, 54, 81, 144, 218,
            151, 69, 210, 177, 116, 179, 148, 200, 35, 116, 161, 30, 109, 168, 93,
        ],
        base: *b"c845e8ce457adef92f151825f8365190da9745d2b174b394c82374a11e6da85d.fibp",
        map_encoded: [
            99, 56, 52, 53, 101, 56, 99, 101, 52, 53, 55, 97, 100, 101, 102, 57, 50,
            102, 49, 53, 49, 56, 50, 53, 102, 56, 51, 54, 53, 49, 57, 48, 100, 97, 57,
            55, 52, 53, 100, 50, 98, 49, 55, 52, 98, 51, 57, 52, 99, 56, 50, 51, 55, 52,
            97, 49, 49, 101, 54, 100, 97, 56, 53, 100, 255, 255, 255, 255, 0, 0, 0, 0,
            0, 0, 0, 0,
        ],
        map_sha: [
            192, 41, 254, 84, 160, 39, 195, 4, 196, 84, 80, 160, 107, 23, 206, 237, 167,
            243, 243, 48, 18, 59, 60, 221, 177, 85, 166, 17, 98, 226, 111, 102,
        ],
        map: *b"c029fe54a027c304c45450a06b17ceeda7f3f330123b3cddb155a61162e26f66.fibp",
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
        generation: 217317991,
        encoded: [
            4, 21, 38, 55, 72, 89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20,
            37, 54, 71, 88, 105, 122, 139, 156, 173, 190, 207, 224, 241, 2, 19, 206, 43,
            218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147,
            131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205, 111, 91,
            131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117, 240, 33,
            4, 3, 2, 1, 103, 2, 244, 12, 6, 0, 0, 0, 0,
        ],
        sha: [
            39, 77, 124, 252, 193, 88, 230, 91, 229, 187, 86, 8, 8, 167, 178, 9, 160,
            133, 188, 197, 246, 130, 121, 28, 43, 214, 82, 72, 57, 9, 16, 145,
        ],
        base: *b"274d7cfcc158e65be5bb560808a7b209a085bcc5f682791c2bd6524839091091.fibp",
        map_encoded: [
            50, 55, 52, 100, 55, 99, 102, 99, 99, 49, 53, 56, 101, 54, 53, 98, 101, 53,
            98, 98, 53, 54, 48, 56, 48, 56, 97, 55, 98, 50, 48, 57, 97, 48, 56, 53, 98,
            99, 99, 53, 102, 54, 56, 50, 55, 57, 49, 99, 50, 98, 100, 54, 53, 50, 52,
            56, 51, 57, 48, 57, 49, 48, 57, 49, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0,
            0,
        ],
        map_sha: [
            116, 250, 163, 159, 22, 246, 123, 48, 114, 148, 86, 131, 219, 230, 29, 77,
            14, 165, 11, 38, 204, 114, 64, 134, 243, 180, 220, 117, 14, 47, 207, 42,
        ],
        map: *b"74faa39f16f67b3072945683dbe61d4d0ea50b26cc724086f3b4dc750e2fcf2a.fibp",
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
        generation: 217317991,
        encoded: [
            8, 25, 42, 59, 76, 93, 110, 127, 144, 161, 178, 195, 212, 229, 246, 7, 24,
            41, 58, 75, 92, 109, 126, 143, 160, 177, 194, 211, 228, 245, 6, 23, 206, 43,
            218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147,
            131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205, 111, 91,
            131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117, 240, 33,
            0, 0, 0, 0, 103, 2, 244, 12, 6, 0, 0, 0, 0,
        ],
        sha: [
            247, 176, 141, 231, 54, 181, 8, 69, 47, 236, 247, 28, 253, 58, 252, 86, 162,
            45, 158, 167, 246, 31, 111, 235, 166, 48, 44, 184, 84, 32, 92, 113,
        ],
        base: *b"f7b08de736b508452fecf71cfd3afc56a22d9ea7f61f6feba6302cb854205c71.fibp",
        map_encoded: [
            102, 55, 98, 48, 56, 100, 101, 55, 51, 54, 98, 53, 48, 56, 52, 53, 50, 102,
            101, 99, 102, 55, 49, 99, 102, 100, 51, 97, 102, 99, 53, 54, 97, 50, 50,
            100, 57, 101, 97, 55, 102, 54, 49, 102, 54, 102, 101, 98, 97, 54, 51, 48,
            50, 99, 98, 56, 53, 52, 50, 48, 53, 99, 55, 49, 255, 255, 255, 255, 0, 0, 0,
            0, 0, 0, 0, 0,
        ],
        map_sha: [
            78, 223, 176, 58, 208, 71, 61, 20, 150, 47, 172, 38, 76, 223, 135, 67, 12,
            163, 168, 213, 32, 87, 176, 91, 239, 125, 92, 59, 155, 208, 192, 14,
        ],
        map: *b"4edfb03ad0473d14962fac264cdf87430ca3a8d52057b05bef7d5c3b9bd0c00e.fibp",
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
        generation: 217317991,
        encoded: [
            13, 30, 47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29,
            46, 63, 80, 97, 114, 131, 148, 165, 182, 199, 216, 233, 250, 11, 28, 206,
            43, 218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197,
            147, 131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205,
            111, 91, 131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117,
            240, 33, 4, 3, 2, 1, 103, 2, 244, 12, 6, 0, 0, 0, 0,
        ],
        sha: [
            13, 173, 92, 122, 97, 95, 156, 3, 176, 112, 222, 36, 51, 142, 177, 13, 62,
            137, 202, 161, 190, 91, 103, 33, 35, 103, 124, 25, 38, 93, 93, 245,
        ],
        base: *b"0dad5c7a615f9c03b070de24338eb10d3e89caa1be5b672123677c19265d5df5.fibp",
        map_encoded: [
            48, 100, 97, 100, 53, 99, 55, 97, 54, 49, 53, 102, 57, 99, 48, 51, 98, 48,
            55, 48, 100, 101, 50, 52, 51, 51, 56, 101, 98, 49, 48, 100, 51, 101, 56, 57,
            99, 97, 97, 49, 98, 101, 53, 98, 54, 55, 50, 49, 50, 51, 54, 55, 55, 99, 49,
            57, 50, 54, 53, 100, 53, 100, 102, 53, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0,
            0, 0,
        ],
        map_sha: [
            122, 47, 50, 224, 180, 87, 103, 89, 222, 124, 129, 87, 224, 208, 9, 74, 242,
            116, 208, 87, 160, 112, 70, 168, 126, 245, 2, 24, 94, 108, 67, 133,
        ],
        map: *b"7a2f32e0b4576759de7c8157e0d0094af274d057a07046a87ef502185e6c4385.fibp",
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
        generation: 1614278583,
        encoded: [
            10, 27, 44, 61, 78, 95, 112, 129, 146, 163, 180, 197, 214, 231, 248, 9, 26,
            43, 60, 77, 94, 111, 128, 145, 162, 179, 196, 213, 230, 247, 8, 25, 206, 43,
            218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147,
            131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205, 111, 91,
            131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117, 240, 33,
            7, 0, 0, 0, 183, 239, 55, 96, 6, 0, 0, 0, 1,
        ],
        sha: [
            165, 63, 174, 92, 34, 100, 17, 204, 156, 153, 124, 183, 160, 16, 195, 9,
            169, 35, 39, 89, 131, 137, 158, 135, 21, 34, 193, 60, 15, 158, 2, 93,
        ],
        base: *b"a53fae5c226411cc9c997cb7a010c309a923275983899e871522c13c0f9e025d.fibp",
        map_encoded: [
            97, 53, 51, 102, 97, 101, 53, 99, 50, 50, 54, 52, 49, 49, 99, 99, 57, 99,
            57, 57, 55, 99, 98, 55, 97, 48, 49, 48, 99, 51, 48, 57, 97, 57, 50, 51, 50,
            55, 53, 57, 56, 51, 56, 57, 57, 101, 56, 55, 49, 53, 50, 50, 99, 49, 51, 99,
            48, 102, 57, 101, 48, 50, 53, 100, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0,
            0,
        ],
        map_sha: [
            71, 176, 189, 229, 244, 163, 136, 86, 194, 210, 8, 199, 12, 45, 38, 129,
            137, 76, 151, 40, 145, 97, 178, 40, 34, 146, 242, 255, 111, 248, 169, 61,
        ],
        map: *b"47b0bde5f4a38856c2d208c70c2d2681894c97289161b2282292f2ff6ff8a93d.fibp",
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
        generation: 1614278583,
        encoded: [
            14, 31, 48, 65, 82, 99, 116, 133, 150, 167, 184, 201, 218, 235, 252, 13, 30,
            47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 206,
            43, 218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197,
            147, 131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205,
            111, 91, 131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117,
            240, 33, 4, 3, 2, 1, 183, 239, 55, 96, 6, 0, 0, 0, 1,
        ],
        sha: [
            196, 95, 17, 119, 134, 73, 49, 197, 80, 112, 180, 49, 115, 50, 160, 137,
            120, 76, 218, 121, 104, 186, 225, 18, 207, 103, 180, 148, 69, 114, 242, 246,
        ],
        base: *b"c45f1177864931c55070b4317332a089784cda7968bae112cf67b4944572f2f6.fibp",
        map_encoded: [
            99, 52, 53, 102, 49, 49, 55, 55, 56, 54, 52, 57, 51, 49, 99, 53, 53, 48, 55,
            48, 98, 52, 51, 49, 55, 51, 51, 50, 97, 48, 56, 57, 55, 56, 52, 99, 100, 97,
            55, 57, 54, 56, 98, 97, 101, 49, 49, 50, 99, 102, 54, 55, 98, 52, 57, 52,
            52, 53, 55, 50, 102, 50, 102, 54, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0,
            0,
        ],
        map_sha: [
            99, 197, 40, 88, 251, 185, 220, 145, 33, 127, 100, 161, 19, 204, 180, 1, 10,
            163, 219, 81, 249, 82, 59, 145, 30, 91, 146, 235, 30, 227, 82, 218,
        ],
        map: *b"63c52858fbb9dc91217f64a113ccb4010aa3db51f9523b911e5b92eb1ee352da.fibp",
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
        generation: 1614278583,
        encoded: [
            19, 36, 53, 70, 87, 104, 121, 138, 155, 172, 189, 206, 223, 240, 1, 18, 35,
            52, 69, 86, 103, 120, 137, 154, 171, 188, 205, 222, 239, 0, 17, 34, 206, 43,
            218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147,
            131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205, 111, 91,
            131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117, 240, 33,
            7, 0, 0, 0, 183, 239, 55, 96, 6, 0, 0, 0, 1,
        ],
        sha: [
            102, 161, 85, 48, 66, 235, 199, 188, 176, 49, 243, 52, 180, 56, 218, 80, 53,
            50, 3, 2, 110, 49, 108, 5, 195, 214, 111, 42, 216, 88, 232, 150,
        ],
        base: *b"66a1553042ebc7bcb031f334b438da50353203026e316c05c3d66f2ad858e896.fibp",
        map_encoded: [
            54, 54, 97, 49, 53, 53, 51, 48, 52, 50, 101, 98, 99, 55, 98, 99, 98, 48, 51,
            49, 102, 51, 51, 52, 98, 52, 51, 56, 100, 97, 53, 48, 51, 53, 51, 50, 48,
            51, 48, 50, 54, 101, 51, 49, 54, 99, 48, 53, 99, 51, 100, 54, 54, 102, 50,
            97, 100, 56, 53, 56, 101, 56, 57, 54, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0,
            0, 0,
        ],
        map_sha: [
            236, 127, 10, 98, 85, 7, 120, 109, 255, 78, 90, 129, 49, 123, 234, 1, 230,
            174, 206, 22, 145, 208, 63, 46, 183, 212, 121, 74, 225, 118, 208, 26,
        ],
        map: *b"ec7f0a625507786dff4e5a81317bea01e6aece1691d03f2eb7d4794ae176d01a.fibp",
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
        generation: 1614278583,
        encoded: [
            13, 30, 47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29,
            46, 63, 80, 97, 114, 131, 148, 165, 182, 199, 216, 233, 250, 11, 28, 206,
            43, 218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197,
            147, 131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205,
            111, 91, 131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117,
            240, 33, 4, 3, 2, 1, 183, 239, 55, 96, 6, 0, 0, 0, 1,
        ],
        sha: [
            195, 218, 54, 248, 8, 213, 25, 152, 136, 250, 132, 234, 108, 229, 62, 198,
            202, 251, 181, 117, 100, 125, 16, 212, 197, 92, 234, 188, 181, 32, 93, 207,
        ],
        base: *b"c3da36f808d5199888fa84ea6ce53ec6cafbb575647d10d4c55ceabcb5205dcf.fibp",
        map_encoded: [
            99, 51, 100, 97, 51, 54, 102, 56, 48, 56, 100, 53, 49, 57, 57, 56, 56, 56,
            102, 97, 56, 52, 101, 97, 54, 99, 101, 53, 51, 101, 99, 54, 99, 97, 102, 98,
            98, 53, 55, 53, 54, 52, 55, 100, 49, 48, 100, 52, 99, 53, 53, 99, 101, 97,
            98, 99, 98, 53, 50, 48, 53, 100, 99, 102, 255, 255, 255, 255, 0, 0, 0, 0, 0,
            0, 0, 0,
        ],
        map_sha: [
            222, 56, 215, 199, 40, 211, 255, 174, 243, 62, 222, 18, 51, 174, 239, 242,
            34, 220, 153, 143, 237, 96, 6, 101, 245, 20, 26, 165, 56, 24, 124, 48,
        ],
        map: *b"de38d7c728d3ffaef33ede1233aeeff222dc998fed600665f5141aa538187c30.fibp",
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
        generation: 1614278583,
        encoded: [
            17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 16,
            33, 50, 67, 84, 101, 118, 135, 152, 169, 186, 203, 220, 237, 254, 15, 32,
            206, 43, 218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228,
            197, 147, 131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83,
            205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197,
            117, 240, 33, 0, 0, 0, 0, 183, 239, 55, 96, 6, 0, 0, 0, 1,
        ],
        sha: [
            238, 27, 90, 85, 22, 63, 71, 193, 2, 198, 108, 151, 139, 185, 185, 211, 28,
            104, 36, 183, 133, 98, 124, 130, 186, 250, 139, 234, 244, 20, 55, 15,
        ],
        base: *b"ee1b5a55163f47c102c66c978bb9b9d31c6824b785627c82bafa8beaf414370f.fibp",
        map_encoded: [
            101, 101, 49, 98, 53, 97, 53, 53, 49, 54, 51, 102, 52, 55, 99, 49, 48, 50,
            99, 54, 54, 99, 57, 55, 56, 98, 98, 57, 98, 57, 100, 51, 49, 99, 54, 56, 50,
            52, 98, 55, 56, 53, 54, 50, 55, 99, 56, 50, 98, 97, 102, 97, 56, 98, 101,
            97, 102, 52, 49, 52, 51, 55, 48, 102, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0,
            0, 0,
        ],
        map_sha: [
            1, 95, 44, 174, 68, 134, 198, 245, 87, 237, 188, 179, 93, 62, 239, 249, 60,
            52, 231, 205, 142, 187, 53, 4, 139, 227, 197, 124, 212, 38, 29, 67,
        ],
        map: *b"015f2cae4486c6f557edbcb35d3eeff93c34e7cd8ebb35048be3c57cd4261d43.fibp",
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
        generation: 1614278583,
        encoded: [
            22, 39, 56, 73, 90, 107, 124, 141, 158, 175, 192, 209, 226, 243, 4, 21, 38,
            55, 72, 89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 37, 206, 43,
            218, 116, 11, 0, 49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147,
            131, 184, 174, 26, 100, 177, 111, 211, 250, 157, 145, 245, 83, 205, 111, 91,
            131, 28, 143, 173, 137, 151, 35, 182, 212, 6, 88, 111, 0, 197, 117, 240, 33,
            4, 3, 2, 1, 183, 239, 55, 96, 6, 0, 0, 0, 1,
        ],
        sha: [
            172, 60, 53, 1, 6, 229, 29, 89, 207, 145, 219, 22, 122, 142, 141, 175, 2, 8,
            53, 66, 139, 99, 209, 116, 24, 172, 50, 37, 55, 217, 198, 207,
        ],
        base: *b"ac3c350106e51d59cf91db167a8e8daf020835428b63d17418ac322537d9c6cf.fibp",
        map_encoded: [
            97, 99, 51, 99, 51, 53, 48, 49, 48, 54, 101, 53, 49, 100, 53, 57, 99, 102,
            57, 49, 100, 98, 49, 54, 55, 97, 56, 101, 56, 100, 97, 102, 48, 50, 48, 56,
            51, 53, 52, 50, 56, 98, 54, 51, 100, 49, 55, 52, 49, 56, 97, 99, 51, 50, 50,
            53, 51, 55, 100, 57, 99, 54, 99, 102, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0,
            0, 0,
        ],
        map_sha: [
            51, 82, 51, 11, 136, 64, 34, 161, 1, 120, 176, 83, 254, 49, 192, 176, 90,
            227, 91, 150, 206, 174, 100, 80, 225, 171, 115, 228, 89, 153, 131, 41,
        ],
        map: *b"3352330b884022a10178b053fe31c0b05ae35b96ceae6450e1ab73e459998329.fibp",
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
        // The slim bar and refresh cadence apply at show time, so every choice
        // shares the same pages (#190).
        for (slim_progress_bar, refresh_pages) in [(false, 1), (true, 5), (false, 10)] {
            let shown = ViewSettings { slim_progress_bar, refresh_pages, ..v.settings };
            let mut h = Hash { v, calls: 0, fail: 0 };
            assert_eq!(identity(shown, &v.digest, v.spine, &mut h, BookLanguage::default()),
                Ok((v.base, v.generation)));
        }
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
    fn with_index<R>(&mut self, _: &[u8; 32], _: impl FnOnce(&mut dyn crate::index_build::Store) -> R) -> Option<R> { None }
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
    // Every slim bar and refresh choice is stored: only the page layout gates.
    for v in VECTORS {
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
                    text_size: if v.settings.text_size == TextSize::Px30 { TextSize::Px22 } else { TextSize::Px30 },
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
