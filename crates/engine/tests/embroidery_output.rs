//! What the embroidery export writes, pinned byte for byte: the sha256 of every export, in DST,
//! EXP, PES and JEF, of documents built here and of the real designs' artwork.
//!
//! A design's bytes come from the stitch planner (`vectorcraft_stitch::plan`), the order the engine
//! sews the objects in, and the format writers. A change to any of them changes these bytes, so it
//! fails here and is made on purpose: the failure lists each export that changed and prints the
//! listing as it now stands, to paste below along with the change. A refactor that must not change
//! what is sewn keeps every hash.
//!
//! - The documents built here, always: fills, wide and narrow strokes, a compound path with a hole,
//!   clip groups (one inside another), symbols, a group and an object that ask for a trim after
//!   them, an opened design's literal stitch runs (as they are, clipped, and with cuts at its
//!   blocks' ends), and the export's options away from their defaults.
//! - The real designs' artwork (`cargo xtask corpus --embroidery`, checked against its manifest),
//!   digitized at the size of the person's design as `embroidery_corpus.rs` does. Local runs skip an
//!   absent corpus; `EMBROIDERY_CORPUS_REQUIRED=1` requires it.
//!
//! A PES or JEF export is also pinned apart from its thread numbers: the hash of its bytes with the
//! thread numbers blanked, and the numbers themselves. A change that only gives some blocks other
//! threads (a palette's table or the matching rule) moves the hashes of the whole files and those
//! numbers, never the blanked hash, and the failure says so export by export.
//!
//! Only the hashes are kept here, never the files. They were recorded on macOS (aarch64). The
//! planner calls the platform's sine, cosine and power functions, whose last bit may differ in
//! another maths library: there a needle point can round to the next 0.1 mm, and a hash differ with
//! no change to the code.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::thread::Palette;
use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};

#[path = "../../../xtask/src/sha256.rs"]
mod sha256;

/// The documents built here ([`documents`]), as `sha256sum` lists files: the hash, two spaces, the
/// document's name and the format's extension.
const SYNTHETIC: &str = "
71cd9afb0fca475aed5c4d241a59c4da0efa11a21e820024c03b024f24e59cf2  fills.dst
48e479fed9c97885efa0cb5a23cedd1562f21c8ea00ce3da234d9e218614ee5e  fills.exp
6c984b2ec7123d9641e08e3c7e1d7035dd67f36120f2f4994959913c1286a26d  fills.pes
0ad3c32be994471ec3fd17c0379f9c10d2f0f916cfe81e6ebf2487e3005b84af  fills.jef
781dcd0206a94a7d8b8ef21868d9fe2f40b6be06d7accdf791b5657776125460  strokes.dst
4dc8987b8419f8a675631f659bef658a852429ab2608d4c0652a9ab3202dd3a7  strokes.exp
709766872b65e5570896866fb59ad8234944359e15f08e57d6c8aeb7e8453108  strokes.pes
782c9e20ccf857cf4814c7b20e68adccf82057964f037adbd45c9f1a4e3cbf40  strokes.jef
c9e180dd033b677e9159c8409185d04906f59880c4de63b8edd2454af3f001cb  compound-path-with-a-hole.dst
6eeb8fc77450233e9b598274189424b8bcb6cd8565a6a1669ab50f90a87cf7d1  compound-path-with-a-hole.exp
09029c7e160599cc0bca78623e661b4df8183e2f6ad41a07c94e5f05a298c0e3  compound-path-with-a-hole.pes
1c936ef36919dc251c793409621af960fea072430bf270378c3278317339f136  compound-path-with-a-hole.jef
f7152fc66bf48744dce86f5556c2a1631a88a77dff1f1cc09857a2122fae2b0e  clip-groups.dst
88eeee00a5641ddaee652b3d1c3b06ca8f1d4c0edd09d0285dab36cb93cd61ff  clip-groups.exp
c0a8b0c10ba13c71a477bc1d95ea35753e72d7e840bc548fc0bfef2dd538b031  clip-groups.pes
525c0f64f3660c7af17acbc3001e67f547a0a452e67a43a1f6e0893af8f7e3cc  clip-groups.jef
2ef76f9a8469c9ca20fc8ef0dd864de4694ba50d0c62a992937ebb4c9b3c3ed0  symbols.dst
a3fb3391a3dc35af8ed87c19f7a3f77ab9c184e963a0e9b3f14bcecddc989ff2  symbols.exp
11ef1bd383cfbb8a31cf12a04aacdcf8b7a689a046f0f7c9c9c6e9a4e2555d2d  symbols.pes
b9afafb1061a64649cb1cb57f4eb491dfff770cd29ba1991099397d8b0ccfb57  symbols.jef
0aac0e1762f49b2e06a80d6798058acd642eae97ae676a6c5fc94a6939eba66c  trim-after.dst
db3dd37acd9bdb7a233814f096ba5b1ba6a66b2cc7b44ce5b95f8444d669eb50  trim-after.exp
8e02299d7c710a2c36c37731db2e768a18c50bb99606489ba5aaf40f02ba9936  trim-after.pes
3fef6c163b2cb9a68e60a8c992ed412926618235bb4951526fc82e210b4aa060  trim-after.jef
3770f862cd00ff1348368f702baca727ce4c7c3df61d0f6c9b7733694bfb7150  opened-design.dst
411ef67683c67f89b56c4f7517dc7824050e421a0fff3a3741e8c24abfdb8f2f  opened-design.exp
a682623753133ca0caab1f7e6ac48cf7299a587dd8619a1f5be593fe6e2ae5d9  opened-design.pes
f5f2844f1c6a4161f60fd41957b5892ea45d8032c14a713e1200525c8ddddf67  opened-design.jef
08ea1acb0ca703d79a5a6aff95df967dcb8a7e885d256c0848fad34288a6c192  opened-design-clipped.dst
0bc5398662184fed8442f0aa225ddd91a9d432e1f3fec4f2eaa672ed56fc4ec9  opened-design-clipped.exp
b648162355c0625a2b88c9b13ae75939c0dcaa4fa0548347fc3dddffed465abb  opened-design-clipped.pes
c0ed172b8a9eaf762ef246168e5266365b8757dfd87f660149e9f160b7360c45  opened-design-clipped.jef
60779f82a2627325eca8218f44549ba02b67a8384e13430efdd7e61f7d4c3f71  opened-design-cut-at-block-ends.dst
0d21bdeaf446854ad17e517fd98c3fb1cde79dcf16e1182b8438c63c80673bb6  opened-design-cut-at-block-ends.exp
0ca2d5b8b8db3cedada9a6505644a784f77c1664a117c65b4ffd1e85b02e0374  opened-design-cut-at-block-ends.pes
d1f49d8273152b6064340076ddab9b45d4b3a2077864814a477ef799534ec262  opened-design-cut-at-block-ends.jef
f8554bb0c22deb3d77faa14b94c417d85dacb16530ad7f48d13040afeda920c1  mixed-stitch-style-fill.dst
63c94dcf51ff773159a34f3ffa270da693018f78febd954c22afd0554514e4e3  mixed-stitch-style-fill.exp
06e14353b1d6e726b675488dcfada8e070f6f43b5f97e6b3a5879adeee04d86f  mixed-stitch-style-fill.pes
7a7518dd5948604c300bf1532d236d627a01e38e44e3772b7ce4c14658b9fbf2  mixed-stitch-style-fill.jef
5994e1310478ba976f3db9a33cf68d390ff35c9e5b3e5cd26b131b023d59e62c  mixed-stitch-style-satin.dst
c25a33e508b130173a56bf8a14ccb59c6fd8ef648bf69327c83588b85abcaad5  mixed-stitch-style-satin.exp
bb6588469c6d556f522a74556e58a5f1dcad75a91fd2ddcab7aee9d94d9504e1  mixed-stitch-style-satin.pes
50ba5e920f6a6ca3f240013005c306d80ab5caa160749977e093273a2391e8f5  mixed-stitch-style-satin.jef
ac3236ff8f71c0801c2bf5d22185633c4e453cea1b6383bae2e64eecb99933dd  mixed-stitch-style-running.dst
73e51eb6717a71e52022825238269f9b8f8dcde58cb59b0bb0b7c6df42558193  mixed-stitch-style-running.exp
664b7de8e53303ce3624538ff1977de54a21847146323c04fcd92b2f3ab67a68  mixed-stitch-style-running.pes
7c614001107a9398f52245e67bf497597178eabb562d3cc9d240b294d79a748a  mixed-stitch-style-running.jef
628a42577dcafcc93cd6b1e4f9a433beba292374ab4f1b2f13eb3596b81ec5c6  mixed-every-option.dst
b825e470d14bce6a48b8222a5184f5a25671c30438629c84cf8273e773d44c9f  mixed-every-option.exp
96f462243eabc04877c73efc0a94b925eccec7d3f755bc736fa8ca150723ed76  mixed-every-option.pes
dccdb00e425381135d60c3ce3f80ae78917d74fa4fc9964b01e646c203bee6d3  mixed-every-option.jef
f1360e29ffdbffbf6c40d265ff6fee22a136b3ce8bbeb7a56fc3f01541d194a6  mixed-shrunk.dst
0a104f6e7e7ba412e57d73129f3e033c912ac4f3d6a5afaa646328274703030f  mixed-shrunk.exp
1417f8a6c82766a66792f4ac0d144feb63648893c7f71a9c3a0e245aa6963c41  mixed-shrunk.pes
70f1819ad4d9de4c11576771831e2ddc3dbd30183177e697ce5225cdef891988  mixed-shrunk.jef
";

/// The real designs' artwork at the person's size ([`DESIGNS`]), listed as [`SYNTHETIC`] is.
const ARTWORK: &str = "
66117e41cc9494c46174ca97acaa9d966d4ccd47b11299ccd0e9695927019989  bullseye-twemoji.dst
80f93b52105ac8b7a5202f8c7e5b97e016c9618a23955253cf0892a15e38d2e5  bullseye-twemoji.exp
ddff29b70870648dbffa01c8ee98c49c68274114b1605f1c794131320dfa6895  bullseye-twemoji.pes
889f9f1c894523c3fc1e0e641bdf551cb4804346bf02239692a72676e16fb973  bullseye-twemoji.jef
4cf9c0f8c8dd217d5d3882fc7b8faeb3764254c07de1c698d935e609d33214f0  fox-twemoji.clipart.dst
2511a55c908af2653bf0acf386f8ae68ea943e35aab0e8fbb033212b549e42d3  fox-twemoji.clipart.exp
04113dc5498c89923e2ca4a0e7c38a8e51acc58f363cb3c5475abbc4d3ed09b7  fox-twemoji.clipart.pes
ead2e31faee31d7a8de63bec91f0cb422be563e0d50a627f7061367825674136  fox-twemoji.clipart.jef
7cb8031943f4228344ed0415ca2d061269f171a8dff30eb2afc18a9979658971  heart-suit-twemoji.clipart.dst
d505ae5ca0e445823b4ef58a1af1e06dc59304ddccd826e99923e52ec4a9329c  heart-suit-twemoji.clipart.exp
366b8cecc0c38c4dc2ad1d2eb4533a7b285b68b4331cd68a51cdc5a8ba5eca2e  heart-suit-twemoji.clipart.pes
0521334653d7f2592c1444c07aa597466ad8efb62e6a50690c384654a8a624a7  heart-suit-twemoji.clipart.jef
592e56ebca2a3106271789cc641fc68f60cc3c45b3370d9df23c20d4d51831f6  joystick-twemoji.dst
6979bef6e710634887f206a95faa11ef781aced99aa6a086095e6fe978e3ca10  joystick-twemoji.exp
961fa811268df7e8d4a07c9d9ca84e18b2f8914bcc4796464956979da90e2fb9  joystick-twemoji.pes
e676cac004b9bc6731c6ce39b7c3050935190b6fcee1a46ebe92fe89277ceee3  joystick-twemoji.jef
a341dabfb46c084fb6879c651c0d8a219aa24386734be319373a337f05e6f748  pool-8-ball-twemoji.dst
88d56f86fa2f6827353074884fa544b2171235d47e2710b7b2244b75b5c04f97  pool-8-ball-twemoji.exp
98faa78a8ed67e35244de7faa42072185571cdfdb54d44975adf6d7b87a17480  pool-8-ball-twemoji.pes
e8f3df0f9df762f76b954c785e8a5ea4c10f3bd05b12135dfbae5e54f0c0a240  pool-8-ball-twemoji.jef
62ee4d5cbb70ddfab2594642ccda8dece153a706b31019683276df974d060b4b  puzzle-piece-twemoji.dst
2e70a6ee136df1e68ebe388131e20d5ab68c95ba53d4c6407f91cd7a34dbb0b2  puzzle-piece-twemoji.exp
3dab591f415666fe4d94177dfb2e949a48c1ae8835c624998c0ed8b8488b49df  puzzle-piece-twemoji.pes
5c96fb1b5e454aeadc7039b6d3f13e07ee4b3beb79e5c8d5027423923f0f0a6e  puzzle-piece-twemoji.jef
438c173f980524b83dfc0c58ad29149b89d1684b1a9ca8fd1361f470046a181a  smiling-face-with-smiling-eyes-twemoji.clipart.dst
296e8a552f52dc95745205436fd9b413edd28f97e69be3fcab07f108d29515de  smiling-face-with-smiling-eyes-twemoji.clipart.exp
757de8facf97bbe80f48aa5592fef2f3488c7b0bfeac2fd34b82b160abb6c18f  smiling-face-with-smiling-eyes-twemoji.clipart.pes
f11ea652b99c65b2de3b1b26bf2697546756f86c2eb82fddee1735972475f820  smiling-face-with-smiling-eyes-twemoji.clipart.jef
6fe71459fb99479ade7b676db843fe802dc419380e4740f5d3ce4adb89d3580d  sos-button-fluent.clipart.dst
4dd6f6f0d087a68adcb8945d5bbf8fc8f8e8e3d0e98f3d913321fa8bd5386ea3  sos-button-fluent.clipart.exp
4945ab0941c8f046f7d700f0a33a07926a07f17e03bdbc1dad394496369c760f  sos-button-fluent.clipart.pes
ec6c20bc86be17c139ec6a59b045167762374ca9d02a42566f7d3158d719466a  sos-button-fluent.clipart.jef
654c5ecddf7ece28673f6769d131c73c88478998bff2b9d44aeb70fa367ac62e  video-game-twemoji.dst
b25db9a38d1d6dc3695f3891fc394223b11a7403097a67e8c14fe404b0542a28  video-game-twemoji.exp
4cd9e45526965c0d1c957784fc81f11e12de76b6da06d5a0dcb3905b8078297c  video-game-twemoji.pes
0f6e33c5b97fd7e837397f384a98bd0c4f63a6c284e56e0d98e70540c811fc43  video-game-twemoji.jef
2484cbafcf1719125dea0a74ea395775175a71c77d26622c98fe24c6fbd874f4  watermelon-twemoji.clipart.dst
ac706c4fb5dc6d97861e9db1446df00c442c951a8ecfbff77dfa5daba95206bb  watermelon-twemoji.clipart.exp
ef556001ff353833349bca4886bdb7d8106fc6779abd5367add6427560a4020a  watermelon-twemoji.clipart.pes
53a34717a3171b12842c1e8d4663cec11fa278a958d3cf5c41d93e5d22942b8b  watermelon-twemoji.clipart.jef
ff2cfff1e53754f2e8ddc895ba78eb82a29aa5508d89d721c6a95379748b36c4  zebra-twemoji.clipart.dst
e4521143ee2d35458c2854d910bf3f098763ad89b27fe7aba31c786f6360476c  zebra-twemoji.clipart.exp
add034d064379cbf73c7ff51969ffff890c888bcd07e7a61c98294ccf93e5a69  zebra-twemoji.clipart.pes
86a5cea2fcf39d066579480e7c0c359e261919b6ae1ca62b5e1422fbeae40c4e  zebra-twemoji.clipart.jef
";

/// The PES and JEF exports of [`SYNTHETIC`] apart from their thread numbers ([`threads_apart`]):
/// each one's hash with its thread numbers blanked, its name, and the numbers, in sewing order.
const SYNTHETIC_THREADS: &str = "
6d85580eeae030376519ce2c91cd836c7a3e261cc45d7e0d789c5cb8db99ca6e  fills.pes  5 21 37 5
3bba2e628dee41e9333883647b51909e577534748fd3bb4cccf5ad49533e7bbe  fills.jef  33 59 45 33
11d0a4feb1f4cd562b0187479d4bde33a7b956ca33ab7266b09fa9aec39f3ce6  strokes.pes  5 21 7 37 52 24
0f23f8f6c391096553cca21a3f6c0fd6872c0d51b55dbf3d69cddbd5c59b03d7  strokes.jef  33 59 59 45 71 14
0f55156f0c7ef89f46abc7204d4ac77c359e3aba2441d707bea8f48fd620b20b  compound-path-with-a-hole.pes  5 21
4dc237a56c66b14d036cc0f8e718f531b36b3b477c01e01331ef3a8cbc32ee8d  compound-path-with-a-hole.jef  33 59
f7989a704d3f568b5d0510c2a95af4f66acaeac50c9e874aea18bd566595ceca  clip-groups.pes  58 5 21 37 23
2253ed4207281892bd546b0eb0417aa09bf6c19616f8136ccf52d9a3d24e2774  clip-groups.jef  37 33 59 45 1
4bd1d95ff46329d5b41465c323c615ed0ea21830df60756a0fc9c6927fed4cf1  symbols.pes  5 21
a8193f55ac29094f84cbc67af92d02a9ab204db6c9b2d6c3a7d072d475d0ee99  symbols.jef  33 59
fa06f35c38957b16c0eb05b441c30ed98a42fe503c4f986efba06ed86a5ec63e  trim-after.pes  5
192823d8623f7f3fbce9089fb9318d004feeeb8ac79ec4cd5cf4b3c192d9f9c7  trim-after.jef  33
5463b0595b08a8a16753031131619342e73ea55ad89134e65a54f53150fbbfd6  opened-design.pes  5 21
f8d65a62ebb85a12f403cd2d222e44896e2e196929108e3138203e854d98d6f1  opened-design.jef  33 12
43772e24f79d5e16a4688bb3bd78b15b835cbdd8b8e3ba06fcefde3b0fd96c03  opened-design-clipped.pes  5
be992ebec030b544d10545200539fafa55a4015393c5a38553cedf7238ca503d  opened-design-clipped.jef  33
9c0456079bba220714bc4c9cc30928a48ad7bf6d42cb109966f5b24b6aa31c63  opened-design-cut-at-block-ends.pes  5 55
e6effa654bfa5ad9d95db6df0c25e8de2686eed3f5ce3e9d6fa10dc8c1715f10  opened-design-cut-at-block-ends.jef  33 59
6f6efd9f4ee8e9a34d01f1521dcc57f1ccf4a07f12cbd6a1431c54fcaf883126  mixed-stitch-style-fill.pes  5 37 21 7 52 24
4fb4120120ccfe3ca857008d9539379d15eee3ff589b3998397317f201e9a374  mixed-stitch-style-fill.jef  33 45 59 59 71 14
851f9636ff0682d36e287c85e2e6e45e39094cd9ac1e0604df60093eadbe0518  mixed-stitch-style-satin.pes  5 37 21 7 52 24
9701ed54fa71c7abfa23fca5e9b6531ffce7afaeb9c5a935449aad64428f74bd  mixed-stitch-style-satin.jef  33 45 59 59 71 14
f055106c44518010de1226db210a08f3293dc055b1b64ac915d0d99ffe5a0fce  mixed-stitch-style-running.pes  5 37 21 7 52 24
022898c2d8df04d76dcc1a81b2fb306e030cb6a14dfdb9b228f2ffe4d69013ae  mixed-stitch-style-running.jef  33 45 59 59 71 14
c048456b0420f849ea094c7a7722329ad55c7179d79383f61f40ebea6a192d69  mixed-every-option.pes  5 37 21 7 52 24
77e8b0cd5af5df68c0e2c934d2f1ca1553572c4fd2ab2fb602dd6cbfb1f47810  mixed-every-option.jef  33 45 59 59 71 14
eae62169ecdc54a9123c8d61ac986e4964cf56345b05b17849754a6d13be5516  mixed-shrunk.pes  5 37 21 7 52 24
68bb5e902b8fe6968a38da6b1ebc3d132c83d438bd234b0b5c7a2763da6752f7  mixed-shrunk.jef  33 45 59 59 71 14
";

/// The PES and JEF exports of [`ARTWORK`] apart from their thread numbers, listed as
/// [`SYNTHETIC_THREADS`] is.
const ARTWORK_THREADS: &str = "
0527952da68e1a56eca25b22f95c71026a3f3fa1f2fa411914f75849359d31d7  bullseye-twemoji.pes  29 32 5 33 20 36
45c126406bec505658e0434cac29838aa66a8121c437da355c2b14d8a5dcc542  bullseye-twemoji.jef  23 28 33 72 1 11
ad00a31720a9a0f36ee338f08248895ad5cf496355711048596cf729b7b03c51  fox-twemoji.clipart.pes  52 12 20 51
436a5dd5417b7aa497eb5e43e06fa012ed3ab7eddfad2d10ababe15a3a76f2fb  fox-twemoji.clipart.jef  71 13 1 42
c42660ea6f3f62c28e16e77b32c262173236f89141d2a0439b2f00c6ec393291  heart-suit-twemoji.clipart.pes  51
3a582db45cd3c3908ee333d943b2c2a509f997b21fc7e26752a8b06313f76e05  heart-suit-twemoji.clipart.jef  42
c0afa92e4c6966ab680077fbc22d17945d80437c0992d5ac34afb828a508215c  joystick-twemoji.pes  16 48 23 39
f476081b2158eac9b1dbe2657420ab152052639a42ad03cec521bf37bdb295e6  joystick-twemoji.jef  21 33 50 50
27e2658254542a92fbc6fc620ca46775d8dc2449454f2e6bf31a8996d09cd5a0  pool-8-ball-twemoji.pes  23 29 23
3a10d758edeab9ce9be44b30e5b251118a450410953521136421bc320b407dd7  pool-8-ball-twemoji.jef  50 23 50
0666920eb3f75fe49602c67f6d93b08acbd49ec07f9c7553375fdd7f51dfb8bb  puzzle-piece-twemoji.pes  37
9a222e0ddc1914f91f07e5b08f32b8e1851a4eeb99182b5cf092638e48ca8637  puzzle-piece-twemoji.jef  22
1a688cd77eb918579f06522b619aa9e6ee2b00d80438394fb33fedd0669bfe86  smiling-face-with-smiling-eyes-twemoji.clipart.pes  58 43 23
d9352300ecee75a7018d15506102c727c8a2fd4e900727cca1fbac1f4b4c133b  smiling-face-with-smiling-eyes-twemoji.clipart.jef  73 9 1
f7e159c9fc72d844c582edee56118026975f4a3259715cc33f4edc1a9fd2a733  sos-button-fluent.clipart.pes  48 29
e9ca7ca008223660401fced648d91a3651d77badf02a520054a8938e247103ca  sos-button-fluent.clipart.jef  32 2
2e17b2fbd6af7c627264a1457ab89c08608f64cbec5782bd3b02d0910e28a651  video-game-twemoji.pes  23 20 39 12 37 32 5 39
91c8ba74cae597feccbfdd0c4d5b225ce808fb6008d1b5df34b783ee5167ac0d  video-game-twemoji.jef  50 1 50 37 22 28 33 24
6f59ff87ff9764a5db53aac26f86fe086a243fc9d6c64778cac2f3b16793ae78  watermelon-twemoji.clipart.pes  54 29 5 20
fc1c386b8c347c703e9a6947af98a0770a85b6eb23c85977dfd78bab01cee641  watermelon-twemoji.clipart.jef  6 51 42 1
223f7e65b78c7fb1f42cf4bee2a6f6cea04cab88ed74bf74bd36662416c66e60  zebra-twemoji.clipart.pes  29 20 29 39 23 20 39 20
2853d6536561088d04462d9cdaed53bdd8d48d8f9895f06a9155eb9a8dd55ea5  zebra-twemoji.clipart.jef  51 1 51 50 1 1 50 1
";

/// The real designs: each artwork, and the machine file a person digitized from it, whose size the
/// artwork is scaled to (as `embroidery_corpus.rs` scales it).
const DESIGNS: &[(&str, &str)] = &[
    ("bullseye-twemoji.svg", "bullseye-twemoji-inkstitch.pes"),
    ("fox-twemoji.clipart.svg", "fox-twemoji-inkstitch.pes"),
    ("heart-suit-twemoji.clipart.svg", "heart-suit-twemoji-inkstitch.pes"),
    ("joystick-twemoji.svg", "joystick-twemoji-institch.pes"),
    ("pool-8-ball-twemoji.svg", "pool-8-ball-twemoji-inkstitch.pes"),
    ("puzzle-piece-twemoji.svg", "puzzle-piece-twemoji-inkstitch.pes"),
    ("smiling-face-with-smiling-eyes-twemoji.clipart.svg", "smiling-face-with-smiling-eyes-twemoji-inkstitch.pes"),
    ("sos-button-fluent.clipart.svg", "sos-button-fluent-inkstitch.pes"),
    ("video-game-twemoji.svg", "video-game-twemoji-inkstitch.pes"),
    ("watermelon-twemoji.clipart.svg", "watermelon-twemoji-inkstitch.pes"),
    ("zebra-twemoji.clipart.svg", "zebra-twemoji-inkstitch.pes"),
];

/// Points per millimetre: the documents count points, the designs millimetres.
const PT_PER_MM: f64 = 72.0 / 25.4;

/// Where the listings were recorded (`std::env::consts::OS` and `ARCH`): a difference on another
/// platform may come from its maths library rather than the code, and the failure says so.
const RECORDED_ON: &str = "macos aarch64";

/// A letter T, its corners in millimetres: its strokes are 3 mm wide, narrow enough to be sewn as
/// satin columns, and the square where they meet is tatami.
const LETTER_T: [(f64, f64); 8] = [(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)];

// Where the thread numbers are in the files our writers write (the module docs of
// `vectorcraft_stitch::format::pes` and `::jef`).

/// Where a PES header gives the PEC section's position (a little-endian word).
const PEC_POSITION: usize = 8;
/// Where the PEC header holds its colour count, one fewer than the colours ([`NO_COLORS`]: none);
/// one palette index a byte follows for each colour.
const PEC_COLOR_COUNT: usize = 48;
const NO_COLORS: u8 = 0xFF;
/// Where a JEF header gives its colour count (a little-endian word), and where its palette indices
/// start, a word each.
const JEF_COLOR_COUNT: usize = 24;
const JEF_THREADS: usize = 0x74;
/// A JEF palette index's length, bytes.
const WORD: usize = 4;

/// The documents built here, exported in every format, write what they wrote when the listing was
/// recorded.
#[test]
fn the_documents_built_here_export_unchanged() {
    let mut now = Vec::new();
    for (name, mut s, options) in documents() {
        for format in Format::ALL {
            let bytes = export(&mut s, format, &options).unwrap_or_else(|e| panic!("{name} as {}: {e}", format.id()));
            now.push(Pinned::new(format!("{name}.{}", format.extension()), format, &bytes));
        }
    }
    check(SYNTHETIC, SYNTHETIC_THREADS, &now);
}

/// One export as the listings pin it.
struct Pinned {
    /// The document's name and the format's extension.
    name: String,
    /// The hash of its bytes.
    hash: String,
    /// For a format that numbers its threads (PES, JEF): the hash of its bytes with the thread
    /// numbers blanked, and the numbers ([`threads_apart`]).
    threads: Option<(String, String)>,
}

impl Pinned {
    fn new(name: String, format: Format, bytes: &[u8]) -> Self {
        let threads = threads_apart(format, bytes)
            .map(|(blanked, numbers)| (sha256::hex(&blanked), numbers.iter().map(u32::to_string).collect::<Vec<_>>().join(" ")));
        Self { name, hash: sha256::hex(bytes), threads }
    }
}

/// `bytes`, as our writer of `format` wrote them, with their thread numbers set to 0, and the
/// numbers, in sewing order: the PEC header's palette indices in a PES file, the header's palette
/// indices in a JEF file. `None` for a format that numbers no threads.
fn threads_apart(format: Format, bytes: &[u8]) -> Option<(Vec<u8>, Vec<u32>)> {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + WORD].try_into().unwrap()) as usize;
    let (first, count, width) = match format {
        Format::Pes => {
            let header = word(PEC_POSITION);
            let count = match bytes[header + PEC_COLOR_COUNT] {
                NO_COLORS => 0,
                c => usize::from(c) + 1,
            };
            (header + PEC_COLOR_COUNT + 1, count, 1)
        }
        Format::Jef => (JEF_THREADS, word(JEF_COLOR_COUNT), WORD),
        Format::Dst | Format::Exp => return None,
    };
    let mut blanked = bytes.to_vec();
    let mut numbers = Vec::with_capacity(count);
    for at in (0..count).map(|i| first + i * width) {
        let field = &mut blanked[at..at + width];
        numbers.push(field.iter().rev().fold(0u32, |n, b| n << 8 | u32::from(*b)));
        field.fill(0);
    }
    Some((blanked, numbers))
}

/// The real designs' artwork, digitized at the person's size in every format, writes what it wrote
/// when the listing was recorded.
#[test]
fn the_real_designs_artwork_exports_unchanged() {
    let Some(dir) = corpus() else { return };
    let mut now = Vec::new();
    for &(art, theirs) in DESIGNS {
        let person = Format::Pes.read(&std::fs::read(dir.join(theirs)).unwrap()).unwrap();
        let target = person.bounds().unwrap();
        let mut s = Session::new();
        open(&mut s, art, &std::fs::read(dir.join(art)).unwrap());
        // At the person's size: the art scaled so that its stitched size matches the design's.
        let first = Format::Pes.read(&export(&mut s, Format::Pes, &json!({})).unwrap()).unwrap().bounds().unwrap();
        let k = (target.width() / first.width()).max(target.height() / first.height());
        run(&mut s, "select.all", json!({}));
        run(&mut s, "object.scale", json!({"sx": k * 100.0, "sy": k * 100.0, "strokes": true}));
        let stem = art.trim_end_matches(".svg");
        for format in Format::ALL {
            let bytes = export(&mut s, format, &json!({})).unwrap_or_else(|e| panic!("{art} as {}: {e}", format.id()));
            now.push(Pinned::new(format!("{stem}.{}", format.extension()), format, &bytes));
        }
    }
    check(ARTWORK, ARTWORK_THREADS, &now);
}

/// One design written with two sets of threads gives two files that are the same apart from their
/// thread numbers ([`threads_apart`]), which read back as the threads the blocks are painted with.
#[test]
fn a_design_in_other_threads_differs_only_in_its_thread_numbers() {
    for (format, palette, firsts, seconds) in
        [(Format::Pes, Palette::Pec, [5, 20, 5], [13, 29, 40]), (Format::Jef, Palette::Jef, [10, 1, 10], [3, 2, 61])]
    {
        let written = |numbers: [usize; 3]| {
            let blocks = numbers.iter().enumerate().map(|(k, &n)| {
                // A thread's own colour is that thread ([`Palette::nearest`]).
                let mut block = Block::new(Thread::rgb(palette.get(n).unwrap().0));
                let y = 5.0 * k as f64;
                block.stitches.extend((0..10).map(|i| Stitch::new(f64::from(i) * 2.0, y, Command::Stitch)));
                block
            });
            format.write(&Pattern { name: "threads".into(), blocks: blocks.collect() }).unwrap()
        };
        let (a, b) = (written(firsts), written(seconds));
        assert_ne!(a, b, "{}", format.id());
        let ((a_blanked, a_numbers), (b_blanked, b_numbers)) = (threads_apart(format, &a).unwrap(), threads_apart(format, &b).unwrap());
        assert_eq!(a_blanked, b_blanked, "{}", format.id());
        assert_eq!((a_numbers, b_numbers), (firsts.map(|n| n as u32).to_vec(), seconds.map(|n| n as u32).to_vec()));
    }
    assert_eq!(threads_apart(Format::Dst, &[]), None);
}

/// The lines of a listing, each cut at its double spaces: hash, name and, in a listing of thread
/// numbers, the numbers.
fn lines(listing: &str) -> Vec<Vec<&str>> {
    listing.lines().map(str::trim).filter(|l| !l.is_empty()).map(|l| l.split("  ").collect()).collect()
}

/// Every export's hash is the one `listing` records, and every PES and JEF export's bytes apart
/// from its thread numbers, and the numbers, the ones `threads` records; the listings have no
/// other. Otherwise the failure names each change, says whether an export changed only in its
/// thread numbers, and prints both listings as they now stand.
fn check(listing: &str, threads: &str, now: &[Pinned]) {
    let recorded = lines(listing);
    let recorded_threads = lines(threads);
    fn find<'a>(lines: &[Vec<&'a str>], name: &str) -> Option<Vec<&'a str>> {
        lines.iter().find(|l| l.get(1) == Some(&name)).cloned()
    }
    let mut changed: Vec<String> = Vec::new();
    for p in now {
        let mut notes: Vec<String> = Vec::new();
        match find(&recorded, &p.name) {
            Some(l) if l[0] == p.hash => {}
            Some(l) => notes.push(format!("{} → {}", l[0], p.hash)),
            None => notes.push(format!("not recorded → {}", p.hash)),
        }
        if let Some((blanked, numbers)) = &p.threads {
            match find(&recorded_threads, &p.name) {
                Some(l) if l[0] != blanked => notes.push(format!("more than its thread numbers: without them, {} → {blanked}", l[0])),
                Some(l) => {
                    let was = l.get(2).copied().unwrap_or("");
                    if was != numbers {
                        notes.push(format!("only its thread numbers: {was} → {numbers}"));
                    }
                }
                None => notes.push(format!("its thread numbers not recorded → {blanked}  {numbers}")),
            }
        }
        if !notes.is_empty() {
            changed.push(format!("{}: {}", p.name, notes.join("; ")));
        }
    }
    for l in recorded.iter().chain(&recorded_threads) {
        if let Some(name) = l.get(1)
            && !now.iter().any(|p| p.name == *name)
        {
            changed.push(format!("{name}: {} → no longer exported", l[0]));
        }
    }
    let listing: String = now.iter().map(|p| format!("{}  {}\n", p.hash, p.name)).collect();
    let threads: String =
        now.iter().filter_map(|p| p.threads.as_ref().map(|(blanked, numbers)| format!("{blanked}  {}  {numbers}\n", p.name))).collect();
    let here = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let platform = if here == RECORDED_ON {
        String::new()
    } else {
        format!(
            "\n\nThe listing was recorded on {RECORDED_ON} and this is {here}: its sine, cosine and power functions may round a needle point to the next 0.1 mm, so compare the stitches before recording a new listing."
        )
    };
    assert!(
        changed.is_empty(),
        "{} exports changed:\n{}{platform}\n\nThe listing as it now stands:\n{listing}\nApart from the thread numbers:\n{threads}",
        changed.len(),
        changed.join("\n")
    );
}

/// The documents built here: each one's name, a session holding it, and the options it is exported
/// with.
fn documents() -> Vec<(&'static str, Session, Value)> {
    let every_option = json!({
        "density": 0.5, "stitchLength": 3, "stitchAngle": 30, "pullCompensation": [0.3, 0.1], "pullCompensationPercent": 10,
        "pushCompensation": [0.5, 0.2], "expand": 0.4, "narrowAsSatin": 5, "threadWeight": 60, "underlay": false,
        "jumpLength": 3, "trimLength": 6, "minStitch": 0.3
    });
    vec![
        ("fills", fills(), json!({})),
        ("strokes", strokes(), json!({})),
        ("compound-path-with-a-hole", compound(), json!({})),
        ("clip-groups", clip_groups(), json!({})),
        ("symbols", symbols(), json!({})),
        ("trim-after", trim_after(), json!({})),
        ("opened-design", opened(), json!({})),
        ("opened-design-clipped", opened_and_clipped(), json!({})),
        ("opened-design-cut-at-block-ends", opened_with_cuts(), json!({})),
        ("mixed-stitch-style-fill", mixed(), json!({"stitchStyle": "fill"})),
        ("mixed-stitch-style-satin", mixed(), json!({"stitchStyle": "satin"})),
        ("mixed-stitch-style-running", mixed(), json!({"stitchStyle": "running"})),
        ("mixed-every-option", mixed(), every_option),
        ("mixed-shrunk", mixed(), json!({"expand": -0.3, "pullCompensation": 0})),
    ]
}

/// Fills: a rectangle and an ellipse, a letter T (its strokes narrow enough to be sewn as satin
/// columns), a rectangle of the first one's thread far from it (sewn in its block, after a trim),
/// and one of that thread on top of the ellipse (a block of its own). A hidden rectangle and a
/// template layer's are not sewn.
fn fills() -> Session {
    let mut s = blank(300.0, 300.0);
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    ellipse(&mut s, "#2233cc", 90.0, 30.0, 30.0, 30.0);
    filled_mm(&mut s, "#22aa44", (150.0, 120.0), &[&LETTER_T]);
    rect(&mut s, "#cc2222", 20.0, 200.0, 30.0, 20.0);
    rect(&mut s, "#cc2222", 100.0, 40.0, 20.0, 20.0);
    let hidden = rect(&mut s, "#000000", 200.0, 20.0, 40.0, 40.0);
    run(&mut s, "object.hide", json!({"ids": [hidden]}));
    let template = run(&mut s, "layer.new", json!({}))["id"].clone();
    rect(&mut s, "#ff8800", 220.0, 220.0, 40.0, 40.0);
    run(&mut s, "layer.setProps", json!({"id": template, "template": true}));
    s
}

/// Strokes: a satin line, a satin ellipse (a closed column), a satin S curve, running lines (a
/// zigzag of sharp corners, a curve), and a rectangle both filled and stroked (two parts of one
/// object).
fn strokes() -> Session {
    let mut s = blank(300.0, 300.0);
    stroked(&mut s, "#cc2222", 6.0, "M 20 30 L 200 30");
    let ring = ellipse(&mut s, "#2233cc", 40.0, 60.0, 120.0, 60.0);
    run(&mut s, "paint.setFill", json!({"none": true, "ids": [ring]}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc", "ids": [ring]}));
    run(&mut s, "stroke.set", json!({"weight": 8.0, "ids": [ring]}));
    stroked(&mut s, "#8822aa", 10.0, "M 190 150 C 260 150 200 220 270 220");
    stroked(&mut s, "#22aa44", 1.0, "M 20 200 L 60 160 L 100 200 L 140 160 L 180 200");
    stroked(&mut s, "#22aa44", 0.5, "M 20 280 C 80 230 120 300 180 250");
    let both = rect(&mut s, "#ff8800", 200.0, 30.0, 60.0, 60.0);
    run(&mut s, "paint.setStroke", json!({"color": "#552200", "ids": [both]}));
    run(&mut s, "stroke.set", json!({"weight": 4.0, "ids": [both]}));
    s
}

/// A compound path with a hole (a square in a square), and one of two squares apart (two pieces).
fn compound() -> Session {
    let mut s = blank(300.0, 300.0);
    let outer = rect(&mut s, "#cc2222", 40.0, 40.0, 80.0, 80.0);
    let hole = rect(&mut s, "#cc2222", 70.0, 70.0, 20.0, 20.0);
    run(&mut s, "select.set", json!({"ids": [outer, hole]}));
    run(&mut s, "object.compoundPath.make", json!({}));
    run(&mut s, "select.none", json!({}));
    let a = rect(&mut s, "#2233cc", 150.0, 40.0, 30.0, 30.0);
    let b = rect(&mut s, "#2233cc", 220.0, 40.0, 30.0, 30.0);
    run(&mut s, "select.set", json!({"ids": [a, b]}));
    run(&mut s, "object.compoundPath.make", json!({}));
    run(&mut s, "select.none", json!({}));
    s
}

/// A clip group (a fill, a running line and a satin line, cut by a square whose own fill and stroke
/// are sewn under and over them) inside another, clipped by an ellipse.
fn clip_groups() -> Session {
    let mut s = blank(300.0, 300.0);
    rect(&mut s, "#cc2222", 0.0, 0.0, 200.0, 200.0);
    stroked(&mut s, "#2233cc", 1.0, "M 0 75 L 250 75");
    stroked(&mut s, "#22aa44", 6.0, "M 75 0 L 75 250");
    let square = rect(&mut s, "#000000", 50.0, 50.0, 100.0, 100.0);
    run(&mut s, "select.all", json!({}));
    run(&mut s, "object.clippingMask.make", json!({}));
    run(&mut s, "select.none", json!({}));
    run(&mut s, "paint.setFill", json!({"color": "#eecc00", "ids": [square]}));
    run(&mut s, "paint.setStroke", json!({"color": "#222222", "ids": [square]}));
    run(&mut s, "stroke.set", json!({"weight": 3.0, "ids": [square]}));
    ellipse(&mut s, "#000000", 30.0, 30.0, 120.0, 120.0);
    run(&mut s, "select.all", json!({}));
    run(&mut s, "object.clippingMask.make", json!({}));
    run(&mut s, "select.none", json!({}));
    s
}

/// A symbol (a square under a satin ring) and three instances of it: where it was made, moved, and
/// rotated and scaled.
fn symbols() -> Session {
    let mut s = blank(300.0, 300.0);
    let square = rect(&mut s, "#cc2222", 20.0, 20.0, 30.0, 30.0);
    let ring = ellipse(&mut s, "#2233cc", 25.0, 25.0, 20.0, 20.0);
    run(&mut s, "paint.setFill", json!({"none": true, "ids": [ring]}));
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc", "ids": [ring]}));
    run(&mut s, "stroke.set", json!({"weight": 4.0, "ids": [ring]}));
    run(&mut s, "select.set", json!({"ids": [square, ring]}));
    let name = run(&mut s, "symbol.new", json!({"name": "Badge"}))["name"].clone();
    run(&mut s, "select.none", json!({}));
    run(&mut s, "symbol.place", json!({"name": name, "x": 150, "y": 60}));
    let turned = run(&mut s, "symbol.place", json!({"name": name, "x": 100, "y": 200}))["id"].clone();
    run(&mut s, "object.rotate", json!({"ids": [turned], "angle": 30}));
    run(&mut s, "object.scale", json!({"ids": [turned], "sx": 150, "sy": 150, "strokes": true}));
    run(&mut s, "select.none", json!({}));
    s
}

/// Four squares of one thread a short jump apart: the first two grouped, the group and the third
/// square asking for the thread to be cut after them.
fn trim_after() -> Session {
    let mut s = blank(300.0, 300.0);
    let squares: Vec<u64> = [20.0, 40.0, 60.0, 80.0].into_iter().map(|x| rect(&mut s, "#cc2222", x, 20.0, 15.0, 15.0)).collect();
    run(&mut s, "select.set", json!({"ids": [squares[0], squares[1]]}));
    let group = run(&mut s, "object.group", json!({}))["id"].clone();
    run(&mut s, "select.none", json!({}));
    run(&mut s, "embroidery.setTrimAfter", json!({"on": true, "ids": [group, squares[2]]}));
    s
}

/// Two threads, one in two shapes far apart, written as PES and opened: literal stitch runs, with
/// the file's jumps and trims between them.
fn opened() -> Session {
    let mut s = blank(300.0, 300.0);
    rect(&mut s, "#cc2222", 20.0, 20.0, 40.0, 30.0);
    ellipse(&mut s, "#2233cc", 90.0, 30.0, 30.0, 30.0);
    rect(&mut s, "#cc2222", 20.0, 120.0, 30.0, 20.0);
    let pes = export(&mut s, Format::Pes, &json!({})).unwrap();
    open(&mut s, "design.pes", &pes);
    s
}

/// The opened design ([`opened`]) inside a clip group: its runs cut to the clipping path.
fn opened_and_clipped() -> Session {
    let mut s = opened();
    run(&mut s, "paint.setFill", json!({"none": true}));
    run(&mut s, "paint.setStroke", json!({"none": true}));
    run(&mut s, "shape.rectangle", json!({"x": 10, "y": 10, "width": 25, "height": 40}));
    run(&mut s, "select.all", json!({}));
    run(&mut s, "object.clippingMask.make", json!({}));
    run(&mut s, "select.none", json!({}));
    s
}

/// A design written in code with cuts where Ink/Stitch puts them, at the end of a block (before the
/// thread is changed) and at the start of the next, and a cut between two runs, opened from PES.
fn opened_with_cuts() -> Session {
    let line = |y: f64, from: i32, to: i32| (from..to).map(move |i| Stitch::new(f64::from(i) * 2.0, y, Command::Stitch));
    let mut a = Block::new(Thread::rgb([200, 0, 0]));
    a.stitches.extend(line(0.0, 0, 10));
    a.stitches.push(Stitch::new(30.0, 0.0, Command::Jump));
    a.stitches.extend(line(0.0, 15, 25));
    a.stitches.push(Stitch::new(48.0, 0.0, Command::Trim));
    let mut b = Block::new(Thread::rgb([0, 0, 200]));
    b.stitches.push(Stitch::new(0.0, 10.0, Command::Trim));
    b.stitches.extend(line(10.0, 0, 10));
    b.stitches.push(Stitch::new(18.0, 10.0, Command::Trim));
    b.stitches.push(Stitch::new(40.0, 10.0, Command::Jump));
    b.stitches.extend(line(10.0, 20, 30));
    let pattern = Pattern { name: "cuts".into(), blocks: vec![a, b] };
    let mut s = Session::new();
    open(&mut s, "cuts.pes", &Format::Pes.write(&pattern).unwrap());
    s
}

/// Fills and strokes together, for the options: a rectangle, a letter T, a satin line, a running
/// zigzag and a filled and stroked square.
fn mixed() -> Session {
    let mut s = blank(300.0, 300.0);
    rect(&mut s, "#cc2222", 20.0, 20.0, 60.0, 40.0);
    filled_mm(&mut s, "#22aa44", (110.0, 20.0), &[&LETTER_T]);
    stroked(&mut s, "#2233cc", 6.0, "M 20 120 C 80 90 140 150 200 120");
    stroked(&mut s, "#8822aa", 1.0, "M 20 200 L 60 160 L 100 200 L 140 160 L 180 200");
    let both = rect(&mut s, "#ff8800", 200.0, 180.0, 60.0, 60.0);
    run(&mut s, "paint.setStroke", json!({"color": "#552200", "ids": [both]}));
    run(&mut s, "stroke.set", json!({"weight": 4.0, "ids": [both]}));
    s
}

fn blank(width: f64, height: f64) -> Session {
    let mut s = Session::new();
    run(&mut s, "file.new", json!({"width": width, "height": height, "artboards": 1}));
    s
}

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|e| panic!("{command} {params}: {e}"))
}

fn id(created: &Value) -> u64 {
    created["id"].as_u64().unwrap_or_else(|| panic!("no id in {created}"))
}

/// A filled, unstroked rectangle in `color` (points), left unselected → its id.
fn rect(s: &mut Session, color: &str, x: f64, y: f64, width: f64, height: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": color}));
    let id = id(&run(s, "shape.rectangle", json!({"x": x, "y": y, "width": width, "height": height})));
    run(s, "select.none", json!({}));
    id
}

/// A filled, unstroked ellipse in `color` (points), left unselected → its id.
fn ellipse(s: &mut Session, color: &str, x: f64, y: f64, width: f64, height: f64) -> u64 {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": color}));
    let id = id(&run(s, "shape.ellipse", json!({"x": x, "y": y, "width": width, "height": height})));
    run(s, "select.none", json!({}));
    id
}

/// An unfilled path stroked in `color`, `weight` points wide, from SVG path data in points, left
/// unselected.
fn stroked(s: &mut Session, color: &str, weight: f64, d: &str) {
    run(s, "paint.setFill", json!({"none": true}));
    run(s, "paint.setStroke", json!({"color": color}));
    let id = id(&run(s, "path.create", json!({"d": d})));
    run(s, "stroke.set", json!({"weight": weight, "ids": [id]}));
    run(s, "select.none", json!({}));
}

/// A filled, unstroked path in `color` of closed polygons with corners in millimetres, placed at
/// `at` (points), left unselected.
fn filled_mm(s: &mut Session, color: &str, at: (f64, f64), polygons: &[&[(f64, f64)]]) {
    run(s, "paint.setStroke", json!({"none": true}));
    run(s, "paint.setFill", json!({"color": color}));
    let mut d = String::new();
    for polygon in polygons {
        for (i, (x, y)) in polygon.iter().enumerate() {
            d.push_str(&format!("{} {} {} ", if i == 0 { "M" } else { "L" }, at.0 + x * PT_PER_MM, at.1 + y * PT_PER_MM));
        }
        d.push_str("Z ");
    }
    run(s, "path.create", json!({"d": d}));
    run(s, "select.none", json!({}));
}

fn export(s: &mut Session, format: Format, options: &Value) -> Result<Vec<u8>, String> {
    let mut p = options.clone();
    p["format"] = json!(format.id());
    let r = s.execute("document.export", &p).map_err(|e| e.to_string())?;
    vectorcraft_format::base64_decode(r["dataBase64"].as_str().ok_or("no dataBase64")?).ok_or_else(|| "dataBase64 is not base64".to_string())
}

fn open(s: &mut Session, name: &str, bytes: &[u8]) {
    run(s, "document.open", json!({"name": name, "dataBase64": vectorcraft_format::base64_encode(bytes)}));
}

/// The real designs, checked against their manifest; `None` (the test skips) when they are absent
/// and not required.
fn corpus() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/embroidery");
    let required = std::env::var_os("EMBROIDERY_CORPUS_REQUIRED").is_some_and(|v| v != "0");
    if !dir.exists() {
        assert!(!required, "required embroidery corpus absent: run `cargo xtask corpus --embroidery`");
        eprintln!("embroidery corpus absent: run `cargo xtask corpus --embroidery`");
        return None;
    }
    for line in include_str!("../../../xtask/embroidery-corpus.sha256").lines().filter(|l| !l.trim().is_empty()) {
        let (digest, name) = line.split_once("  ").expect("malformed embroidery corpus manifest");
        let bytes =
            std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("missing corpus file {name}: {e}; run `cargo xtask corpus --embroidery`"));
        assert_eq!(sha256::hex(&bytes), digest, "{name}: corpus file changed; run `cargo xtask corpus --embroidery`");
    }
    Some(dir)
}
