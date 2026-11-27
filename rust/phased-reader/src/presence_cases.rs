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
        generation: 1254694451,
        encoded: [
            1, 18, 35, 52, 69, 86, 103, 120, 137, 154, 171, 188, 205, 222, 239, 0, 17, 34, 51, 68,
            85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 16, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 7, 0, 0, 0, 51, 30, 201, 74, 5, 0, 0, 0, 0, 0,
            1,
        ],
        sha: [
            83, 148, 91, 40, 67, 173, 213, 130, 166, 75, 163, 211, 224, 192, 115, 118, 205, 212,
            252, 123, 167, 248, 202, 247, 89, 48, 148, 213, 104, 115, 193, 61,
        ],
        base: *b"53945b2843add582a64ba3d3e0c07376cdd4fc7ba7f8caf7593094d56873c13d.fibp",
        map_encoded: [
            53, 51, 57, 52, 53, 98, 50, 56, 52, 51, 97, 100, 100, 53, 56, 50, 97, 54, 52, 98, 97,
            51, 100, 51, 101, 48, 99, 48, 55, 51, 55, 54, 99, 100, 100, 52, 102, 99, 55, 98, 97,
            55, 102, 56, 99, 97, 102, 55, 53, 57, 51, 48, 57, 52, 100, 53, 54, 56, 55, 51, 99, 49,
            51, 100, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            41, 51, 81, 212, 253, 95, 228, 15, 176, 110, 60, 114, 212, 116, 123, 135, 167, 87, 228,
            79, 235, 97, 96, 212, 4, 179, 151, 5, 106, 185, 215, 34,
        ],
        map: *b"293351d4fd5fe40fb06e3c72d4747b87a757e44feb6160d404b397056ab9d722.fibp",
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
        generation: 1321804927,
        encoded: [
            5, 22, 39, 56, 73, 90, 107, 124, 141, 158, 175, 192, 209, 226, 243, 4, 21, 38, 55, 72,
            89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 127, 36, 201, 78, 5, 0, 0, 0, 0, 0,
            5,
        ],
        sha: [
            2, 166, 229, 236, 199, 19, 203, 14, 94, 239, 34, 254, 207, 103, 144, 58, 81, 248, 22,
            201, 113, 83, 152, 223, 158, 12, 255, 139, 133, 81, 134, 21,
        ],
        base: *b"02a6e5ecc713cb0e5eef22fecf67903a51f816c9715398df9e0cff8b85518615.fibp",
        map_encoded: [
            48, 50, 97, 54, 101, 53, 101, 99, 99, 55, 49, 51, 99, 98, 48, 101, 53, 101, 101, 102,
            50, 50, 102, 101, 99, 102, 54, 55, 57, 48, 51, 97, 53, 49, 102, 56, 49, 54, 99, 57, 55,
            49, 53, 51, 57, 56, 100, 102, 57, 101, 48, 99, 102, 102, 56, 98, 56, 53, 53, 49, 56,
            54, 49, 53, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            98, 245, 7, 225, 202, 190, 188, 26, 213, 159, 188, 72, 47, 122, 112, 218, 88, 249, 44,
            181, 62, 83, 169, 29, 142, 93, 166, 138, 104, 62, 190, 88,
        ],
        map: *b"62f507e1cabebc1ad59fbc482f7a70da58f92cb53e53a91d8e5da68a683ebe58.fibp",
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
        generation: 1405693022,
        encoded: [
            10, 27, 44, 61, 78, 95, 112, 129, 146, 163, 180, 197, 214, 231, 248, 9, 26, 43, 60, 77,
            94, 111, 128, 145, 162, 179, 196, 213, 230, 247, 8, 25, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 7, 0, 0, 0, 94, 44, 201, 83, 5, 0, 0, 0, 0, 0,
            10,
        ],
        sha: [
            19, 249, 25, 177, 184, 182, 235, 111, 53, 223, 105, 211, 183, 123, 119, 174, 10, 29,
            11, 160, 165, 172, 226, 148, 175, 9, 71, 4, 180, 230, 26, 167,
        ],
        base: *b"13f919b1b8b6eb6f35df69d3b77b77ae0a1d0ba0a5ace294af094704b4e61aa7.fibp",
        map_encoded: [
            49, 51, 102, 57, 49, 57, 98, 49, 98, 56, 98, 54, 101, 98, 54, 102, 51, 53, 100, 102,
            54, 57, 100, 51, 98, 55, 55, 98, 55, 55, 97, 101, 48, 97, 49, 100, 48, 98, 97, 48, 97,
            53, 97, 99, 101, 50, 57, 52, 97, 102, 48, 57, 52, 55, 48, 52, 98, 52, 101, 54, 49, 97,
            97, 55, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            244, 152, 163, 37, 159, 182, 165, 244, 9, 34, 171, 103, 48, 160, 80, 241, 118, 20, 158,
            12, 232, 79, 115, 106, 157, 135, 94, 76, 216, 143, 213, 17,
        ],
        map: *b"f498a3259fb6a5f40922ab6730a050f176149e0ce84f736a9d875e4cd88fd511.fibp",
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
        generation: 1858835830,
        encoded: [
            4, 21, 38, 55, 72, 89, 106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 37, 54, 71,
            88, 105, 122, 139, 156, 173, 190, 207, 224, 241, 2, 19, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 118, 149, 203, 110, 5, 0, 0, 0, 0,
            1, 1,
        ],
        sha: [
            143, 83, 85, 34, 59, 31, 126, 220, 165, 186, 186, 115, 158, 231, 252, 82, 41, 40, 41,
            195, 188, 201, 117, 248, 111, 43, 110, 27, 43, 83, 157, 227,
        ],
        base: *b"8f5355223b1f7edca5baba739ee7fc52292829c3bcc975f86f2b6e1b2b539de3.fibp",
        map_encoded: [
            56, 102, 53, 51, 53, 53, 50, 50, 51, 98, 49, 102, 55, 101, 100, 99, 97, 53, 98, 97, 98,
            97, 55, 51, 57, 101, 101, 55, 102, 99, 53, 50, 50, 57, 50, 56, 50, 57, 99, 51, 98, 99,
            99, 57, 55, 53, 102, 56, 54, 102, 50, 98, 54, 101, 49, 98, 50, 98, 53, 51, 57, 100,
            101, 51, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            236, 171, 21, 194, 3, 5, 76, 229, 2, 50, 133, 42, 59, 128, 202, 107, 112, 162, 86, 110,
            34, 119, 111, 43, 235, 117, 207, 219, 75, 154, 236, 130,
        ],
        map: *b"ecab15c203054ce50232852a3b80ca6b70a2566e22776f2beb75cfdb4b9aec82.fibp",
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
        generation: 1925946306,
        encoded: [
            8, 25, 42, 59, 76, 93, 110, 127, 144, 161, 178, 195, 212, 229, 246, 7, 24, 41, 58, 75,
            92, 109, 126, 143, 160, 177, 194, 211, 228, 245, 6, 23, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 0, 0, 0, 0, 194, 155, 203, 114, 5, 0, 0, 0, 0,
            1, 5,
        ],
        sha: [
            102, 123, 49, 133, 25, 75, 212, 211, 250, 241, 120, 201, 60, 87, 155, 110, 99, 17, 132,
            221, 193, 229, 187, 181, 26, 132, 245, 195, 139, 142, 208, 5,
        ],
        base: *b"667b3185194bd4d3faf178c93c579b6e631184ddc1e5bbb51a84f5c38b8ed005.fibp",
        map_encoded: [
            54, 54, 55, 98, 51, 49, 56, 53, 49, 57, 52, 98, 100, 52, 100, 51, 102, 97, 102, 49, 55,
            56, 99, 57, 51, 99, 53, 55, 57, 98, 54, 101, 54, 51, 49, 49, 56, 52, 100, 100, 99, 49,
            101, 53, 98, 98, 98, 53, 49, 97, 56, 52, 102, 53, 99, 51, 56, 98, 56, 101, 100, 48, 48,
            53, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            163, 255, 124, 139, 30, 236, 206, 140, 187, 38, 240, 178, 134, 127, 249, 212, 39, 101,
            193, 112, 2, 71, 35, 99, 215, 158, 202, 54, 213, 193, 106, 200,
        ],
        map: *b"a3ff7c8b1eecce8cbb26f0b2867ff9d42765c17002472363d79eca36d5c16ac8.fibp",
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
        generation: 1976279163,
        encoded: [
            13, 30, 47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 46, 63,
            80, 97, 114, 131, 148, 165, 182, 199, 216, 233, 250, 11, 28, 206, 43, 218, 116, 11, 0,
            49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 123, 160, 203, 117, 5, 0, 0, 0, 0,
            1, 10,
        ],
        sha: [
            197, 61, 95, 129, 147, 27, 7, 234, 72, 172, 231, 250, 234, 242, 196, 107, 74, 60, 29,
            183, 213, 144, 210, 177, 121, 89, 33, 0, 177, 102, 22, 140,
        ],
        base: *b"c53d5f81931b07ea48ace7faeaf2c46b4a3c1db7d590d2b179592100b166168c.fibp",
        map_encoded: [
            99, 53, 51, 100, 53, 102, 56, 49, 57, 51, 49, 98, 48, 55, 101, 97, 52, 56, 97, 99, 101,
            55, 102, 97, 101, 97, 102, 50, 99, 52, 54, 98, 52, 97, 51, 99, 49, 100, 98, 55, 100,
            53, 57, 48, 100, 50, 98, 49, 55, 57, 53, 57, 50, 49, 48, 48, 98, 49, 54, 54, 49, 54,
            56, 99, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            230, 122, 212, 49, 41, 207, 91, 139, 195, 220, 149, 74, 56, 22, 128, 217, 19, 223, 41,
            62, 4, 127, 4, 122, 241, 48, 200, 195, 30, 131, 163, 41,
        ],
        map: *b"e67ad43129cf5b8bc3dc954a381680d913df293e047f047af130c8c31e83a329.fibp",
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
        generation: 1438858243,
        encoded: [
            10, 27, 44, 61, 78, 95, 112, 129, 146, 163, 180, 197, 214, 231, 248, 9, 26, 43, 60, 77,
            94, 111, 128, 145, 162, 179, 196, 213, 230, 247, 8, 25, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 7, 0, 0, 0, 3, 60, 195, 85, 5, 0, 0, 0, 1, 0, 1,
        ],
        sha: [
            158, 220, 85, 93, 196, 107, 188, 229, 132, 133, 178, 117, 189, 233, 31, 205, 84, 94,
            163, 247, 23, 211, 15, 90, 103, 232, 224, 80, 214, 160, 148, 48,
        ],
        base: *b"9edc555dc46bbce58485b275bde91fcd545ea3f717d30f5a67e8e050d6a09430.fibp",
        map_encoded: [
            57, 101, 100, 99, 53, 53, 53, 100, 99, 52, 54, 98, 98, 99, 101, 53, 56, 52, 56, 53, 98,
            50, 55, 53, 98, 100, 101, 57, 49, 102, 99, 100, 53, 52, 53, 101, 97, 51, 102, 55, 49,
            55, 100, 51, 48, 102, 53, 97, 54, 55, 101, 56, 101, 48, 53, 48, 100, 54, 97, 48, 57,
            52, 51, 48, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            193, 111, 101, 103, 74, 219, 55, 86, 179, 92, 223, 41, 222, 114, 26, 123, 188, 97, 98,
            249, 151, 49, 148, 198, 252, 209, 5, 160, 66, 49, 24, 56,
        ],
        map: *b"c16f65674adb3756b35cdf29de721a7bbc6162f9973194c6fcd105a042311838.fibp",
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
        generation: 1505968719,
        encoded: [
            14, 31, 48, 65, 82, 99, 116, 133, 150, 167, 184, 201, 218, 235, 252, 13, 30, 47, 64,
            81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 206, 43, 218, 116, 11, 0,
            49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 79, 66, 195, 89, 5, 0, 0, 0, 1, 0,
            5,
        ],
        sha: [
            252, 254, 186, 183, 86, 90, 81, 86, 105, 135, 198, 60, 68, 4, 230, 72, 5, 3, 54, 210,
            243, 244, 208, 148, 223, 190, 115, 100, 253, 152, 199, 114,
        ],
        base: *b"fcfebab7565a51566987c63c4404e648050336d2f3f4d094dfbe7364fd98c772.fibp",
        map_encoded: [
            102, 99, 102, 101, 98, 97, 98, 55, 53, 54, 53, 97, 53, 49, 53, 54, 54, 57, 56, 55, 99,
            54, 51, 99, 52, 52, 48, 52, 101, 54, 52, 56, 48, 53, 48, 51, 51, 54, 100, 50, 102, 51,
            102, 52, 100, 48, 57, 52, 100, 102, 98, 101, 55, 51, 54, 52, 102, 100, 57, 56, 99, 55,
            55, 50, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            205, 189, 91, 12, 212, 209, 195, 194, 234, 179, 248, 254, 23, 248, 52, 60, 196, 114,
            34, 37, 14, 167, 169, 6, 209, 218, 50, 126, 97, 211, 171, 91,
        ],
        map: *b"cdbd5b0cd4d1c3c2eab3f8fe17f8343cc47222250ea7a906d1da327e61d3ab5b.fibp",
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
        generation: 1589856814,
        encoded: [
            19, 36, 53, 70, 87, 104, 121, 138, 155, 172, 189, 206, 223, 240, 1, 18, 35, 52, 69, 86,
            103, 120, 137, 154, 171, 188, 205, 222, 239, 0, 17, 34, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 7, 0, 0, 0, 46, 74, 195, 94, 5, 0, 0, 0, 1, 0,
            10,
        ],
        sha: [
            121, 15, 73, 236, 70, 6, 54, 29, 81, 56, 28, 12, 173, 239, 15, 117, 151, 162, 179, 39,
            129, 146, 139, 185, 112, 13, 202, 207, 68, 25, 244, 125,
        ],
        base: *b"790f49ec4606361d51381c0cadef0f7597a2b32781928bb9700dcacf4419f47d.fibp",
        map_encoded: [
            55, 57, 48, 102, 52, 57, 101, 99, 52, 54, 48, 54, 51, 54, 49, 100, 53, 49, 51, 56, 49,
            99, 48, 99, 97, 100, 101, 102, 48, 102, 55, 53, 57, 55, 97, 50, 98, 51, 50, 55, 56, 49,
            57, 50, 56, 98, 98, 57, 55, 48, 48, 100, 99, 97, 99, 102, 52, 52, 49, 57, 102, 52, 55,
            100, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            176, 143, 125, 96, 52, 240, 147, 4, 154, 14, 181, 156, 41, 177, 59, 157, 112, 216, 145,
            246, 102, 106, 53, 192, 185, 128, 98, 57, 193, 67, 6, 14,
        ],
        map: *b"b08f7d6034f093049a0eb59c29b13b9d70d891f6666a35c0b9806239c143060e.fibp",
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
        generation: 2042999622,
        encoded: [
            13, 30, 47, 64, 81, 98, 115, 132, 149, 166, 183, 200, 217, 234, 251, 12, 29, 46, 63,
            80, 97, 114, 131, 148, 165, 182, 199, 216, 233, 250, 11, 28, 206, 43, 218, 116, 11, 0,
            49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 70, 179, 197, 121, 5, 0, 0, 0, 1, 1,
            1,
        ],
        sha: [
            250, 118, 20, 222, 246, 109, 229, 52, 228, 226, 142, 108, 233, 235, 32, 165, 113, 141,
            32, 1, 249, 28, 29, 144, 87, 131, 63, 96, 232, 123, 30, 32,
        ],
        base: *b"fa7614def66de534e4e28e6ce9eb20a5718d2001f91c1d9057833f60e87b1e20.fibp",
        map_encoded: [
            102, 97, 55, 54, 49, 52, 100, 101, 102, 54, 54, 100, 101, 53, 51, 52, 101, 52, 101, 50,
            56, 101, 54, 99, 101, 57, 101, 98, 50, 48, 97, 53, 55, 49, 56, 100, 50, 48, 48, 49,
            102, 57, 49, 99, 49, 100, 57, 48, 53, 55, 56, 51, 51, 102, 54, 48, 101, 56, 55, 98, 49,
            101, 50, 48, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            8, 119, 87, 253, 173, 43, 121, 15, 80, 42, 179, 123, 45, 217, 70, 233, 26, 111, 131,
            227, 67, 102, 34, 83, 199, 199, 189, 172, 101, 164, 107, 7,
        ],
        map: *b"087757fdad2b790f502ab37b2dd946e91a6f83e343662253c7c7bdac65a46b07.fibp",
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
        generation: 2110110098,
        encoded: [
            17, 34, 51, 68, 85, 102, 119, 136, 153, 170, 187, 204, 221, 238, 255, 16, 33, 50, 67,
            84, 101, 118, 135, 152, 169, 186, 203, 220, 237, 254, 15, 32, 206, 43, 218, 116, 11, 0,
            49, 186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 0, 0, 0, 0, 146, 185, 197, 125, 5, 0, 0, 0, 1,
            1, 5,
        ],
        sha: [
            182, 144, 208, 165, 173, 109, 32, 7, 62, 141, 147, 58, 37, 75, 236, 115, 61, 102, 122,
            100, 106, 146, 111, 112, 230, 156, 191, 62, 199, 183, 244, 173,
        ],
        base: *b"b690d0a5ad6d20073e8d933a254bec733d667a646a926f70e69cbf3ec7b7f4ad.fibp",
        map_encoded: [
            98, 54, 57, 48, 100, 48, 97, 53, 97, 100, 54, 100, 50, 48, 48, 55, 51, 101, 56, 100,
            57, 51, 51, 97, 50, 53, 52, 98, 101, 99, 55, 51, 51, 100, 54, 54, 55, 97, 54, 52, 54,
            97, 57, 50, 54, 102, 55, 48, 101, 54, 57, 99, 98, 102, 51, 101, 99, 55, 98, 55, 102,
            52, 97, 100, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            253, 80, 184, 180, 131, 51, 70, 57, 23, 217, 65, 135, 69, 196, 153, 87, 177, 164, 145,
            237, 129, 96, 76, 47, 5, 107, 192, 245, 59, 207, 102, 230,
        ],
        map: *b"fd50b8b48333463917d9418745c49957b1a491ed81604c2f056bc0f53bcf66e6.fibp",
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
        generation: 2160442955,
        encoded: [
            22, 39, 56, 73, 90, 107, 124, 141, 158, 175, 192, 209, 226, 243, 4, 21, 38, 55, 72, 89,
            106, 123, 140, 157, 174, 191, 208, 225, 242, 3, 20, 37, 206, 43, 218, 116, 11, 0, 49,
            186, 65, 184, 100, 231, 60, 157, 17, 7, 228, 197, 147, 131, 184, 174, 26, 100, 177,
            111, 211, 250, 157, 145, 245, 83, 205, 111, 91, 131, 28, 143, 173, 137, 151, 35, 182,
            212, 6, 88, 111, 0, 197, 117, 240, 33, 4, 3, 2, 1, 75, 190, 197, 128, 5, 0, 0, 0, 1, 1,
            10,
        ],
        sha: [
            54, 74, 97, 15, 157, 90, 71, 2, 196, 234, 151, 219, 148, 161, 175, 1, 27, 67, 154, 60,
            137, 174, 243, 51, 222, 182, 206, 34, 111, 93, 124, 6,
        ],
        base: *b"364a610f9d5a4702c4ea97db94a1af011b439a3c89aef333deb6ce226f5d7c06.fibp",
        map_encoded: [
            51, 54, 52, 97, 54, 49, 48, 102, 57, 100, 53, 97, 52, 55, 48, 50, 99, 52, 101, 97, 57,
            55, 100, 98, 57, 52, 97, 49, 97, 102, 48, 49, 49, 98, 52, 51, 57, 97, 51, 99, 56, 57,
            97, 101, 102, 51, 51, 51, 100, 101, 98, 54, 99, 101, 50, 50, 54, 102, 53, 100, 55, 99,
            48, 54, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
        map_sha: [
            147, 203, 7, 100, 180, 75, 145, 229, 195, 154, 48, 212, 123, 66, 3, 252, 253, 189, 104,
            99, 249, 96, 14, 40, 172, 1, 43, 228, 92, 20, 230, 243,
        ],
        map: *b"93cb0764b44b91e5c39a30d47b4203fcfdbd6863f9600e28ac012be45c14e6f3.fibp",
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
