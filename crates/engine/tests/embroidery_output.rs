//! What the embroidery export writes, pinned byte for byte: the sha256 of every export, in every
//! format (DST, TAP, T01, EXP, PES, PEC, JEF, VP3), of documents built here and of the real
//! designs' artwork.
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
//! A PES, PEC or JEF export is also pinned apart from its thread numbers: the hash of its bytes with the
//! thread numbers blanked, and the numbers themselves. A change that only gives some blocks other
//! threads (a palette's table or the matching rule) moves the hashes of the whole files and those
//! numbers, never the blanked hash, and the failure says so export by export.
//!
//! Beside each hash, a summary of what the export holds, read back by our own reader of its format
//! (so an export that doesn't read fails here): each block's needle points, jumps and trims, the
//! design's size, its longest and shortest stitch, and the 1 mm squares its stitches cross. A
//! failure lists the fields that changed, export by export, so that a change can be told from
//! another before a new listing is recorded. Each export's needle points read back are also checked
//! against the plan's (`embroidery.stats`), block by block, wherever no planned stitch is longer
//! than one of the format's records.
//!
//! Only the hashes and summaries are kept here, never the files. They were recorded on macOS
//! (aarch64). The planner calls the platform's sine, cosine and power functions, whose last bit may
//! differ in another maths library: there a needle point can round to the next 0.1 mm, and a hash
//! differ with no change to the code. On another platform an export passes when its summary is
//! within that much of the recorded one.
// Integration tests: unwrapping and panicking on failure is fine here, unlike in shipped code (AGENTS.md › Robustness).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeSet;

use serde_json::{Value, json};
use vectorcraft_engine::Session;
use vectorcraft_stitch::format::Format;
use vectorcraft_stitch::thread::Palette;
use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread, quantize};

mod embroidery_common;
use embroidery_common::{DESIGNS, artwork, corpus, export, open, run, sha256};

/// The documents built here ([`documents`]), as `sha256sum` lists files: the hash, two spaces, the
/// document's name and the format's extension.
const SYNTHETIC: &str = "
71cd9afb0fca475aed5c4d241a59c4da0efa11a21e820024c03b024f24e59cf2  fills.dst
2f1feb70c2a7404330f2bdd52a95bbab9a9fd31dee0e358a50a64006f6dd0927  fills.tap
2f1feb70c2a7404330f2bdd52a95bbab9a9fd31dee0e358a50a64006f6dd0927  fills.t01
48e479fed9c97885efa0cb5a23cedd1562f21c8ea00ce3da234d9e218614ee5e  fills.exp
6c984b2ec7123d9641e08e3c7e1d7035dd67f36120f2f4994959913c1286a26d  fills.pes
b2b3cead5ef9bfc6f93b5c00e2cffaed296e33ce6ea2087216979dddbbe024fe  fills.pec
0ad3c32be994471ec3fd17c0379f9c10d2f0f916cfe81e6ebf2487e3005b84af  fills.jef
e529a72ef95ab3f7b9247865046a8cc78b72258ed810833e2246a8ad337c2cb8  fills.vp3
781dcd0206a94a7d8b8ef21868d9fe2f40b6be06d7accdf791b5657776125460  strokes.dst
0f1b8d675b4fcd377f2b811344397d2afab7c5cc04e6ebf3579600a1de387707  strokes.tap
0f1b8d675b4fcd377f2b811344397d2afab7c5cc04e6ebf3579600a1de387707  strokes.t01
4dc8987b8419f8a675631f659bef658a852429ab2608d4c0652a9ab3202dd3a7  strokes.exp
709766872b65e5570896866fb59ad8234944359e15f08e57d6c8aeb7e8453108  strokes.pes
6d7148ee816cd447c472992e88e191e027ca5109bff465bd07fb53ad2d98ab74  strokes.pec
782c9e20ccf857cf4814c7b20e68adccf82057964f037adbd45c9f1a4e3cbf40  strokes.jef
80624e645c12a9257700bb610543c70650cc8b5be2505f41d0f092a47a0b20f0  strokes.vp3
c9e180dd033b677e9159c8409185d04906f59880c4de63b8edd2454af3f001cb  compound-path-with-a-hole.dst
f5b1fe51e7ddb483653481879e565d11db1ad4ea577973300254e0371ca726cc  compound-path-with-a-hole.tap
f5b1fe51e7ddb483653481879e565d11db1ad4ea577973300254e0371ca726cc  compound-path-with-a-hole.t01
6eeb8fc77450233e9b598274189424b8bcb6cd8565a6a1669ab50f90a87cf7d1  compound-path-with-a-hole.exp
09029c7e160599cc0bca78623e661b4df8183e2f6ad41a07c94e5f05a298c0e3  compound-path-with-a-hole.pes
fe9559f1076fc5483deda446cd37d8e660bf5ec1599c02a62660e066a0b83b5f  compound-path-with-a-hole.pec
1c936ef36919dc251c793409621af960fea072430bf270378c3278317339f136  compound-path-with-a-hole.jef
922b90aa9b1a2a308dc8b38a826ca60a750fb6abbd4468fba306a96fcb3e7641  compound-path-with-a-hole.vp3
f7152fc66bf48744dce86f5556c2a1631a88a77dff1f1cc09857a2122fae2b0e  clip-groups.dst
da046035197a234e4b15c9b27996506dfbc2f8c6dea8c85d2a252fa33ae04444  clip-groups.tap
da046035197a234e4b15c9b27996506dfbc2f8c6dea8c85d2a252fa33ae04444  clip-groups.t01
88eeee00a5641ddaee652b3d1c3b06ca8f1d4c0edd09d0285dab36cb93cd61ff  clip-groups.exp
c0a8b0c10ba13c71a477bc1d95ea35753e72d7e840bc548fc0bfef2dd538b031  clip-groups.pes
bab6ac1084bc1ca03f75b663d0bc9f8332b96c460d82bbed8f0d67f7d5b589ff  clip-groups.pec
525c0f64f3660c7af17acbc3001e67f547a0a452e67a43a1f6e0893af8f7e3cc  clip-groups.jef
08eed05c4028d43e1c5a33561d928ab0d6821b79b2fe894fa90febd3e7772149  clip-groups.vp3
2ef76f9a8469c9ca20fc8ef0dd864de4694ba50d0c62a992937ebb4c9b3c3ed0  symbols.dst
3da3ab3ef70ce5ecd82f0c97cd3098666d5bfe998d1b718a47edf57243c8422a  symbols.tap
3da3ab3ef70ce5ecd82f0c97cd3098666d5bfe998d1b718a47edf57243c8422a  symbols.t01
a3fb3391a3dc35af8ed87c19f7a3f77ab9c184e963a0e9b3f14bcecddc989ff2  symbols.exp
11ef1bd383cfbb8a31cf12a04aacdcf8b7a689a046f0f7c9c9c6e9a4e2555d2d  symbols.pes
2684d3c510c5f1c095609a046b794723966cfb637ce2be524fedf25ee11382ed  symbols.pec
b9afafb1061a64649cb1cb57f4eb491dfff770cd29ba1991099397d8b0ccfb57  symbols.jef
4b9a49c0f0b30ccb377a5c900de35be5c0986abcc420cc24fee00cc62febe539  symbols.vp3
0aac0e1762f49b2e06a80d6798058acd642eae97ae676a6c5fc94a6939eba66c  trim-after.dst
810ad7358d5a6b4f9bacd108e013029bfb3a31814fed33c75a4baddab07fe5fa  trim-after.tap
810ad7358d5a6b4f9bacd108e013029bfb3a31814fed33c75a4baddab07fe5fa  trim-after.t01
db3dd37acd9bdb7a233814f096ba5b1ba6a66b2cc7b44ce5b95f8444d669eb50  trim-after.exp
8e02299d7c710a2c36c37731db2e768a18c50bb99606489ba5aaf40f02ba9936  trim-after.pes
cbdd1e978715ba967425f6c391629c9ad3ab0a45506fa2c4daf4d491aee6b23b  trim-after.pec
3fef6c163b2cb9a68e60a8c992ed412926618235bb4951526fc82e210b4aa060  trim-after.jef
18a9b31e09496349f130efbecd392189bfde4654c92a48d4dcc6d3fff435918d  trim-after.vp3
3770f862cd00ff1348368f702baca727ce4c7c3df61d0f6c9b7733694bfb7150  opened-design.dst
ed3bcb8827c430234947f94f100ceef0e640b5354f74da2b4b1ddb1add1e8a04  opened-design.tap
ed3bcb8827c430234947f94f100ceef0e640b5354f74da2b4b1ddb1add1e8a04  opened-design.t01
411ef67683c67f89b56c4f7517dc7824050e421a0fff3a3741e8c24abfdb8f2f  opened-design.exp
a682623753133ca0caab1f7e6ac48cf7299a587dd8619a1f5be593fe6e2ae5d9  opened-design.pes
cd80b8e141e6f1129d0dd4421b305ac954c6b7d1615f256e470f819bc31cbbf9  opened-design.pec
f5f2844f1c6a4161f60fd41957b5892ea45d8032c14a713e1200525c8ddddf67  opened-design.jef
95ed6272d246443e7a59cd58f9a3f81e708b8b0bccc42fb23df7e37a417052bc  opened-design.vp3
08ea1acb0ca703d79a5a6aff95df967dcb8a7e885d256c0848fad34288a6c192  opened-design-clipped.dst
8c862cb59aec17c1f4d945c22f155a8eb6298d675f6e573d7ac21d3ba810fe11  opened-design-clipped.tap
8c862cb59aec17c1f4d945c22f155a8eb6298d675f6e573d7ac21d3ba810fe11  opened-design-clipped.t01
0bc5398662184fed8442f0aa225ddd91a9d432e1f3fec4f2eaa672ed56fc4ec9  opened-design-clipped.exp
b648162355c0625a2b88c9b13ae75939c0dcaa4fa0548347fc3dddffed465abb  opened-design-clipped.pes
4dd11bbb7be568f4e42432a80ad17cdfebc16c66c202f46e780ba9dae015d150  opened-design-clipped.pec
c0ed172b8a9eaf762ef246168e5266365b8757dfd87f660149e9f160b7360c45  opened-design-clipped.jef
a4a74b42e4b38a670eedf06bb20d6d790376ff13e3507ca0f3c884bff8125635  opened-design-clipped.vp3
60779f82a2627325eca8218f44549ba02b67a8384e13430efdd7e61f7d4c3f71  opened-design-cut-at-block-ends.dst
fce192774f923868bdd5f800fd2afb9e03ed6f0d62c4cd2577b3029f1174830c  opened-design-cut-at-block-ends.tap
fce192774f923868bdd5f800fd2afb9e03ed6f0d62c4cd2577b3029f1174830c  opened-design-cut-at-block-ends.t01
0d21bdeaf446854ad17e517fd98c3fb1cde79dcf16e1182b8438c63c80673bb6  opened-design-cut-at-block-ends.exp
0ca2d5b8b8db3cedada9a6505644a784f77c1664a117c65b4ffd1e85b02e0374  opened-design-cut-at-block-ends.pes
3be4e0bcb1955c9ba6ce68cf760ca66d3f5d5000e1b1fd4c5146f6f7e9e2a7ea  opened-design-cut-at-block-ends.pec
d1f49d8273152b6064340076ddab9b45d4b3a2077864814a477ef799534ec262  opened-design-cut-at-block-ends.jef
d0c5af020602f9e85c6b002d4d122c14fd4e4e84a18897d2f3e620ea08f9af4b  opened-design-cut-at-block-ends.vp3
f8554bb0c22deb3d77faa14b94c417d85dacb16530ad7f48d13040afeda920c1  mixed-stitch-style-fill.dst
8d9c7e47052614a11f4fc514cdb868b1fabb7ecf3dce3277a430abc75ea93374  mixed-stitch-style-fill.tap
8d9c7e47052614a11f4fc514cdb868b1fabb7ecf3dce3277a430abc75ea93374  mixed-stitch-style-fill.t01
63c94dcf51ff773159a34f3ffa270da693018f78febd954c22afd0554514e4e3  mixed-stitch-style-fill.exp
06e14353b1d6e726b675488dcfada8e070f6f43b5f97e6b3a5879adeee04d86f  mixed-stitch-style-fill.pes
c700eadedfc02167edb69aaf550df74be612ae84f89c84e679c3558b5d661ee3  mixed-stitch-style-fill.pec
7a7518dd5948604c300bf1532d236d627a01e38e44e3772b7ce4c14658b9fbf2  mixed-stitch-style-fill.jef
d46468a9da2fe49ef8d177140eab1e5418eba9797af520bf3836eff86f215297  mixed-stitch-style-fill.vp3
5994e1310478ba976f3db9a33cf68d390ff35c9e5b3e5cd26b131b023d59e62c  mixed-stitch-style-satin.dst
5f3a65f9bf9a5f34d183845874eca10ecc09b1e0de6f654c394e42ed8658ef89  mixed-stitch-style-satin.tap
5f3a65f9bf9a5f34d183845874eca10ecc09b1e0de6f654c394e42ed8658ef89  mixed-stitch-style-satin.t01
c25a33e508b130173a56bf8a14ccb59c6fd8ef648bf69327c83588b85abcaad5  mixed-stitch-style-satin.exp
bb6588469c6d556f522a74556e58a5f1dcad75a91fd2ddcab7aee9d94d9504e1  mixed-stitch-style-satin.pes
03ed8a0a1fcbb12f30ca1701e117314ed593cc65cfe97ae35eb4149323bb4229  mixed-stitch-style-satin.pec
50ba5e920f6a6ca3f240013005c306d80ab5caa160749977e093273a2391e8f5  mixed-stitch-style-satin.jef
053446f9082e16c893259670bd2bf3b614518eb6ccfb3c427eb1928b327bb342  mixed-stitch-style-satin.vp3
ac3236ff8f71c0801c2bf5d22185633c4e453cea1b6383bae2e64eecb99933dd  mixed-stitch-style-running.dst
3090211233875177c2cd028d6f0f725937a653ac443e2dc06fbb6584f8eb3cf7  mixed-stitch-style-running.tap
3090211233875177c2cd028d6f0f725937a653ac443e2dc06fbb6584f8eb3cf7  mixed-stitch-style-running.t01
73e51eb6717a71e52022825238269f9b8f8dcde58cb59b0bb0b7c6df42558193  mixed-stitch-style-running.exp
664b7de8e53303ce3624538ff1977de54a21847146323c04fcd92b2f3ab67a68  mixed-stitch-style-running.pes
00e1445051aaacb08f96e18d65c2d6d94c3912edcb9066c411efafb692ba5e14  mixed-stitch-style-running.pec
7c614001107a9398f52245e67bf497597178eabb562d3cc9d240b294d79a748a  mixed-stitch-style-running.jef
9e675c606bcef2c6b421dfcc2b2046697f55a1ebebb9f26e52dd73df0e1bc62f  mixed-stitch-style-running.vp3
628a42577dcafcc93cd6b1e4f9a433beba292374ab4f1b2f13eb3596b81ec5c6  mixed-every-option.dst
2479751ee8f42cb659ce52a6c6037e43e19f8ac041314162676911535e5cbfe6  mixed-every-option.tap
2479751ee8f42cb659ce52a6c6037e43e19f8ac041314162676911535e5cbfe6  mixed-every-option.t01
b825e470d14bce6a48b8222a5184f5a25671c30438629c84cf8273e773d44c9f  mixed-every-option.exp
96f462243eabc04877c73efc0a94b925eccec7d3f755bc736fa8ca150723ed76  mixed-every-option.pes
2f22f6ea2a45479436fd10d0045161764eac60b36426dafa771a6091e59528b6  mixed-every-option.pec
dccdb00e425381135d60c3ce3f80ae78917d74fa4fc9964b01e646c203bee6d3  mixed-every-option.jef
6b1a869fce7d5973e16abcb6c951ef73631f23c139809fedc008611eb4a21e8f  mixed-every-option.vp3
f1360e29ffdbffbf6c40d265ff6fee22a136b3ce8bbeb7a56fc3f01541d194a6  mixed-shrunk.dst
a1351002fae5ad8dc949c236175aacfde66dfc9a8470dd7739f83f2f4e18f0ce  mixed-shrunk.tap
a1351002fae5ad8dc949c236175aacfde66dfc9a8470dd7739f83f2f4e18f0ce  mixed-shrunk.t01
0a104f6e7e7ba412e57d73129f3e033c912ac4f3d6a5afaa646328274703030f  mixed-shrunk.exp
1417f8a6c82766a66792f4ac0d144feb63648893c7f71a9c3a0e245aa6963c41  mixed-shrunk.pes
bb1c2740764c73516c60e6e73217056a7dc2ffd15457bd8fc3410fd932827512  mixed-shrunk.pec
70f1819ad4d9de4c11576771831e2ddc3dbd30183177e697ce5225cdef891988  mixed-shrunk.jef
b8fdec1f486885631f34ba8a8f92f4c2717f55c1a7473d0ca8e2b8e6fedf0cb3  mixed-shrunk.vp3
";

/// The real designs' artwork at the person's size ([`DESIGNS`]), listed as [`SYNTHETIC`] is.
const ARTWORK: &str = "
66117e41cc9494c46174ca97acaa9d966d4ccd47b11299ccd0e9695927019989  bullseye-twemoji.dst
87321dd4ab2cf144634a64687618b6e8772260f00ec57068773fc5083bf374e9  bullseye-twemoji.tap
87321dd4ab2cf144634a64687618b6e8772260f00ec57068773fc5083bf374e9  bullseye-twemoji.t01
80f93b52105ac8b7a5202f8c7e5b97e016c9618a23955253cf0892a15e38d2e5  bullseye-twemoji.exp
ddff29b70870648dbffa01c8ee98c49c68274114b1605f1c794131320dfa6895  bullseye-twemoji.pes
187a416ba4416cda6c451af7bba8259fd85361243060a995c0840046c7966648  bullseye-twemoji.pec
889f9f1c894523c3fc1e0e641bdf551cb4804346bf02239692a72676e16fb973  bullseye-twemoji.jef
a2a9762a841e0c2f5b36e1ea91e8124b04469772175174b8c7327e89fae69da8  bullseye-twemoji.vp3
4cf9c0f8c8dd217d5d3882fc7b8faeb3764254c07de1c698d935e609d33214f0  fox-twemoji.clipart.dst
bce965b0edfa971d17f565e2c449423db3ac9a41d5b6ce12920ec280fbf68175  fox-twemoji.clipart.tap
bce965b0edfa971d17f565e2c449423db3ac9a41d5b6ce12920ec280fbf68175  fox-twemoji.clipart.t01
2511a55c908af2653bf0acf386f8ae68ea943e35aab0e8fbb033212b549e42d3  fox-twemoji.clipart.exp
04113dc5498c89923e2ca4a0e7c38a8e51acc58f363cb3c5475abbc4d3ed09b7  fox-twemoji.clipart.pes
57e0bad4a3dd7c8bdfc8ef18b6c89b7976a12df18c550161b0dcf921911448f8  fox-twemoji.clipart.pec
ead2e31faee31d7a8de63bec91f0cb422be563e0d50a627f7061367825674136  fox-twemoji.clipart.jef
bd61d5f51113515ee6e56d7112ef03f866f5dec0888bf444de9ecbc34bcda812  fox-twemoji.clipart.vp3
7cb8031943f4228344ed0415ca2d061269f171a8dff30eb2afc18a9979658971  heart-suit-twemoji.clipart.dst
0cb4fad915b053c2c32296e10203de143740fdf823709d3dcbe589271dfd02ed  heart-suit-twemoji.clipart.tap
0cb4fad915b053c2c32296e10203de143740fdf823709d3dcbe589271dfd02ed  heart-suit-twemoji.clipart.t01
d505ae5ca0e445823b4ef58a1af1e06dc59304ddccd826e99923e52ec4a9329c  heart-suit-twemoji.clipart.exp
366b8cecc0c38c4dc2ad1d2eb4533a7b285b68b4331cd68a51cdc5a8ba5eca2e  heart-suit-twemoji.clipart.pes
ea5f47c7987ad0227eeecd06efb883f540f852492bf5e826a8edbf4c96483be5  heart-suit-twemoji.clipart.pec
0521334653d7f2592c1444c07aa597466ad8efb62e6a50690c384654a8a624a7  heart-suit-twemoji.clipart.jef
8970981701c650a7abab9adec11b7fa0d17d7b31afc30005ee190c1676094f77  heart-suit-twemoji.clipart.vp3
592e56ebca2a3106271789cc641fc68f60cc3c45b3370d9df23c20d4d51831f6  joystick-twemoji.dst
b3d8d04cdc25c32a515d4071f858d9f7c3e7ebcbfe595e4c18cf01c74e8c583f  joystick-twemoji.tap
b3d8d04cdc25c32a515d4071f858d9f7c3e7ebcbfe595e4c18cf01c74e8c583f  joystick-twemoji.t01
6979bef6e710634887f206a95faa11ef781aced99aa6a086095e6fe978e3ca10  joystick-twemoji.exp
961fa811268df7e8d4a07c9d9ca84e18b2f8914bcc4796464956979da90e2fb9  joystick-twemoji.pes
856fa71f9bebaabb373bf489143733131fff0040faaf024dc97b39661495c8d8  joystick-twemoji.pec
e676cac004b9bc6731c6ce39b7c3050935190b6fcee1a46ebe92fe89277ceee3  joystick-twemoji.jef
1aaccd58627aeddb172bfdad4a28eb8c659463baaffd6cc65b7f83d9b46596e3  joystick-twemoji.vp3
a341dabfb46c084fb6879c651c0d8a219aa24386734be319373a337f05e6f748  pool-8-ball-twemoji.dst
6517ab9976250f288daf039e67af86cad3795999b7ca074e737feab951af7d9b  pool-8-ball-twemoji.tap
6517ab9976250f288daf039e67af86cad3795999b7ca074e737feab951af7d9b  pool-8-ball-twemoji.t01
88d56f86fa2f6827353074884fa544b2171235d47e2710b7b2244b75b5c04f97  pool-8-ball-twemoji.exp
98faa78a8ed67e35244de7faa42072185571cdfdb54d44975adf6d7b87a17480  pool-8-ball-twemoji.pes
69a607613256e9128e836704c342f127ccfdd275ed05d172d761b38b62422963  pool-8-ball-twemoji.pec
e8f3df0f9df762f76b954c785e8a5ea4c10f3bd05b12135dfbae5e54f0c0a240  pool-8-ball-twemoji.jef
afd96694a3a020bc8b7c9d059659b32e3d97bb1d0379d5e37d1d1824f39f4232  pool-8-ball-twemoji.vp3
62ee4d5cbb70ddfab2594642ccda8dece153a706b31019683276df974d060b4b  puzzle-piece-twemoji.dst
bf03a68f82d56de2455d3314864dcb83727d027afda6b6e4bf72b8ada9fedb5a  puzzle-piece-twemoji.tap
bf03a68f82d56de2455d3314864dcb83727d027afda6b6e4bf72b8ada9fedb5a  puzzle-piece-twemoji.t01
2e70a6ee136df1e68ebe388131e20d5ab68c95ba53d4c6407f91cd7a34dbb0b2  puzzle-piece-twemoji.exp
3dab591f415666fe4d94177dfb2e949a48c1ae8835c624998c0ed8b8488b49df  puzzle-piece-twemoji.pes
2a0bb1287cc2f0e3fb6a84aa71ba27b768d4d5fbc6982d9e9bd19dfa8d8d87cd  puzzle-piece-twemoji.pec
5c96fb1b5e454aeadc7039b6d3f13e07ee4b3beb79e5c8d5027423923f0f0a6e  puzzle-piece-twemoji.jef
6338c8861cd7712a2c5896035671c2d279058f5ea8c346d44f1155f1cbf3528a  puzzle-piece-twemoji.vp3
438c173f980524b83dfc0c58ad29149b89d1684b1a9ca8fd1361f470046a181a  smiling-face-with-smiling-eyes-twemoji.clipart.dst
550cb49033701bed11e71686ee35428d4f53cf2c2d0d7042bc7786c8c6dd4d82  smiling-face-with-smiling-eyes-twemoji.clipart.tap
550cb49033701bed11e71686ee35428d4f53cf2c2d0d7042bc7786c8c6dd4d82  smiling-face-with-smiling-eyes-twemoji.clipart.t01
296e8a552f52dc95745205436fd9b413edd28f97e69be3fcab07f108d29515de  smiling-face-with-smiling-eyes-twemoji.clipart.exp
757de8facf97bbe80f48aa5592fef2f3488c7b0bfeac2fd34b82b160abb6c18f  smiling-face-with-smiling-eyes-twemoji.clipart.pes
ee84ea9a3771e5d2455b36000870065cd13eeaa37053a9faae5d6299bca167c6  smiling-face-with-smiling-eyes-twemoji.clipart.pec
f11ea652b99c65b2de3b1b26bf2697546756f86c2eb82fddee1735972475f820  smiling-face-with-smiling-eyes-twemoji.clipart.jef
2a758bffd0639b462247ee04b954b092998d4a5d7db04b4d2bee094612da7f50  smiling-face-with-smiling-eyes-twemoji.clipart.vp3
6fe71459fb99479ade7b676db843fe802dc419380e4740f5d3ce4adb89d3580d  sos-button-fluent.clipart.dst
531274bbe31dfce96addc59e2eb9e68715ae91617b70d4ef08dfbbb322613d76  sos-button-fluent.clipart.tap
531274bbe31dfce96addc59e2eb9e68715ae91617b70d4ef08dfbbb322613d76  sos-button-fluent.clipart.t01
4dd6f6f0d087a68adcb8945d5bbf8fc8f8e8e3d0e98f3d913321fa8bd5386ea3  sos-button-fluent.clipart.exp
4945ab0941c8f046f7d700f0a33a07926a07f17e03bdbc1dad394496369c760f  sos-button-fluent.clipart.pes
91d48dd078b2b23bd02a89d86963bd7705871a004b3cd673bf304bc5ac2e90ec  sos-button-fluent.clipart.pec
ec6c20bc86be17c139ec6a59b045167762374ca9d02a42566f7d3158d719466a  sos-button-fluent.clipart.jef
50c806ec524a4655f382937ab3a7b020a00523756aeadc178656f744af63f94f  sos-button-fluent.clipart.vp3
654c5ecddf7ece28673f6769d131c73c88478998bff2b9d44aeb70fa367ac62e  video-game-twemoji.dst
a98620c9b6862c3153dd01825b6da8f62e30dc49d279d414e514e7b7a5315efc  video-game-twemoji.tap
a98620c9b6862c3153dd01825b6da8f62e30dc49d279d414e514e7b7a5315efc  video-game-twemoji.t01
b25db9a38d1d6dc3695f3891fc394223b11a7403097a67e8c14fe404b0542a28  video-game-twemoji.exp
4cd9e45526965c0d1c957784fc81f11e12de76b6da06d5a0dcb3905b8078297c  video-game-twemoji.pes
49bc1ada7cc93b4929cda888efd4114ba209feac25cc9c7b3f0b6b860b71b300  video-game-twemoji.pec
0f6e33c5b97fd7e837397f384a98bd0c4f63a6c284e56e0d98e70540c811fc43  video-game-twemoji.jef
3820b4d39240fa082dffcbdef22d08f6899886c1e8360b0985be2452ec507811  video-game-twemoji.vp3
2484cbafcf1719125dea0a74ea395775175a71c77d26622c98fe24c6fbd874f4  watermelon-twemoji.clipart.dst
ed614bc985ae05d0d7da61f49981767b6b97be9343d0f2376dd16b8f8d39b40b  watermelon-twemoji.clipart.tap
ed614bc985ae05d0d7da61f49981767b6b97be9343d0f2376dd16b8f8d39b40b  watermelon-twemoji.clipart.t01
ac706c4fb5dc6d97861e9db1446df00c442c951a8ecfbff77dfa5daba95206bb  watermelon-twemoji.clipart.exp
ef556001ff353833349bca4886bdb7d8106fc6779abd5367add6427560a4020a  watermelon-twemoji.clipart.pes
d90aabe83b247ca6d81ed0c70b89440d7d82f9490d214eff7617cbd68ed9946d  watermelon-twemoji.clipart.pec
53a34717a3171b12842c1e8d4663cec11fa278a958d3cf5c41d93e5d22942b8b  watermelon-twemoji.clipart.jef
c2f521e3809ef8d92498732cf0709dffd64b4daf3fddaa0054ef974f7125cdec  watermelon-twemoji.clipart.vp3
ff2cfff1e53754f2e8ddc895ba78eb82a29aa5508d89d721c6a95379748b36c4  zebra-twemoji.clipart.dst
be517a937226befe7a546087e8454844b661dec17dcee1080907430360e76c4b  zebra-twemoji.clipart.tap
be517a937226befe7a546087e8454844b661dec17dcee1080907430360e76c4b  zebra-twemoji.clipart.t01
e4521143ee2d35458c2854d910bf3f098763ad89b27fe7aba31c786f6360476c  zebra-twemoji.clipart.exp
add034d064379cbf73c7ff51969ffff890c888bcd07e7a61c98294ccf93e5a69  zebra-twemoji.clipart.pes
d6cc3efdef8e1ca5619c38777a617a7149a3f39755cc892d9145c164cff5a218  zebra-twemoji.clipart.pec
86a5cea2fcf39d066579480e7c0c359e261919b6ae1ca62b5e1422fbeae40c4e  zebra-twemoji.clipart.jef
6c683d34cc348f49cbca0d9f69e5e113c248305a30a80df029d8af9d3bf4e03d  zebra-twemoji.clipart.vp3
";

/// The PES, PEC and JEF exports of [`SYNTHETIC`] apart from their thread numbers ([`threads_apart`]):
/// each one's hash with its thread numbers blanked, its name, and the numbers, in sewing order.
const SYNTHETIC_THREADS: &str = "
6d85580eeae030376519ce2c91cd836c7a3e261cc45d7e0d789c5cb8db99ca6e  fills.pes  5 21 37 5
b3eb89d978eb54dcaf44bf713624dac397a04f51ecf51d2b234ab83cecb0ae57  fills.pec  5 21 37 5
3bba2e628dee41e9333883647b51909e577534748fd3bb4cccf5ad49533e7bbe  fills.jef  33 59 45 33
11d0a4feb1f4cd562b0187479d4bde33a7b956ca33ab7266b09fa9aec39f3ce6  strokes.pes  5 21 7 37 52 24
4012b67d27edbb8e03aafcf0a877c48380ac930d3140cc3f11301da5c9480fd4  strokes.pec  5 21 7 37 52 24
0f23f8f6c391096553cca21a3f6c0fd6872c0d51b55dbf3d69cddbd5c59b03d7  strokes.jef  33 59 59 45 71 14
0f55156f0c7ef89f46abc7204d4ac77c359e3aba2441d707bea8f48fd620b20b  compound-path-with-a-hole.pes  5 21
cea58aebe9166c8468d2f8cbdcc79281c8c4d437f5f4189dc4703dd607c91316  compound-path-with-a-hole.pec  5 21
4dc237a56c66b14d036cc0f8e718f531b36b3b477c01e01331ef3a8cbc32ee8d  compound-path-with-a-hole.jef  33 59
f7989a704d3f568b5d0510c2a95af4f66acaeac50c9e874aea18bd566595ceca  clip-groups.pes  58 5 21 37 23
6e5c2d517e15e4bfd2d142427525b59089a77d0c240cff277c39255503f385ef  clip-groups.pec  58 5 21 37 23
2253ed4207281892bd546b0eb0417aa09bf6c19616f8136ccf52d9a3d24e2774  clip-groups.jef  37 33 59 45 1
4bd1d95ff46329d5b41465c323c615ed0ea21830df60756a0fc9c6927fed4cf1  symbols.pes  5 21
12cfb6b05a5626413608da54bd72bbc54cdee60bf45156fc2f66d59f80549a10  symbols.pec  5 21
a8193f55ac29094f84cbc67af92d02a9ab204db6c9b2d6c3a7d072d475d0ee99  symbols.jef  33 59
fa06f35c38957b16c0eb05b441c30ed98a42fe503c4f986efba06ed86a5ec63e  trim-after.pes  5
39bf00a900008961516664ddf530d3b3c43217a143537bd920cdfa6e787c1fbd  trim-after.pec  5
192823d8623f7f3fbce9089fb9318d004feeeb8ac79ec4cd5cf4b3c192d9f9c7  trim-after.jef  33
5463b0595b08a8a16753031131619342e73ea55ad89134e65a54f53150fbbfd6  opened-design.pes  5 21
adaa2619ef6ea253bdf52bfcd62d826213a3daa6b6cf398172e9b54c90248e1a  opened-design.pec  5 21
f8d65a62ebb85a12f403cd2d222e44896e2e196929108e3138203e854d98d6f1  opened-design.jef  33 12
43772e24f79d5e16a4688bb3bd78b15b835cbdd8b8e3ba06fcefde3b0fd96c03  opened-design-clipped.pes  5
162c4982575e5e057de6ee4e909008424727934e44787737e7d6b4c5e96feb27  opened-design-clipped.pec  5
be992ebec030b544d10545200539fafa55a4015393c5a38553cedf7238ca503d  opened-design-clipped.jef  33
9c0456079bba220714bc4c9cc30928a48ad7bf6d42cb109966f5b24b6aa31c63  opened-design-cut-at-block-ends.pes  5 55
f1d21d1976aba971e60ab03c8061d095931ef6b71db82b2f509ec342997a6cd2  opened-design-cut-at-block-ends.pec  5 55
e6effa654bfa5ad9d95db6df0c25e8de2686eed3f5ce3e9d6fa10dc8c1715f10  opened-design-cut-at-block-ends.jef  33 59
6f6efd9f4ee8e9a34d01f1521dcc57f1ccf4a07f12cbd6a1431c54fcaf883126  mixed-stitch-style-fill.pes  5 37 21 7 52 24
5edabbbbdc82e09ee9ac4c309e6ec23336d437b3795704284f0ba2a39ad5b80d  mixed-stitch-style-fill.pec  5 37 21 7 52 24
4fb4120120ccfe3ca857008d9539379d15eee3ff589b3998397317f201e9a374  mixed-stitch-style-fill.jef  33 45 59 59 71 14
851f9636ff0682d36e287c85e2e6e45e39094cd9ac1e0604df60093eadbe0518  mixed-stitch-style-satin.pes  5 37 21 7 52 24
679ad0b04784589ce812024993c0f1cf75b5fac745b969fec92c822d8489363d  mixed-stitch-style-satin.pec  5 37 21 7 52 24
9701ed54fa71c7abfa23fca5e9b6531ffce7afaeb9c5a935449aad64428f74bd  mixed-stitch-style-satin.jef  33 45 59 59 71 14
f055106c44518010de1226db210a08f3293dc055b1b64ac915d0d99ffe5a0fce  mixed-stitch-style-running.pes  5 37 21 7 52 24
e11519428c92bf58987a043149918f9bb3f25edfadfe95e719eec21c5ca91b6a  mixed-stitch-style-running.pec  5 37 21 7 52 24
022898c2d8df04d76dcc1a81b2fb306e030cb6a14dfdb9b228f2ffe4d69013ae  mixed-stitch-style-running.jef  33 45 59 59 71 14
c048456b0420f849ea094c7a7722329ad55c7179d79383f61f40ebea6a192d69  mixed-every-option.pes  5 37 21 7 52 24
6e890c887be23463ba2432dd95f33f5543cb3034fa5172317bce3b151d43cc1b  mixed-every-option.pec  5 37 21 7 52 24
77e8b0cd5af5df68c0e2c934d2f1ca1553572c4fd2ab2fb602dd6cbfb1f47810  mixed-every-option.jef  33 45 59 59 71 14
eae62169ecdc54a9123c8d61ac986e4964cf56345b05b17849754a6d13be5516  mixed-shrunk.pes  5 37 21 7 52 24
56757dd0c964ba6d7c101b84386e0871406e120db52e4eca6f6b591ca3aa311c  mixed-shrunk.pec  5 37 21 7 52 24
68bb5e902b8fe6968a38da6b1ebc3d132c83d438bd234b0b5c7a2763da6752f7  mixed-shrunk.jef  33 45 59 59 71 14
";

/// The PES, PEC and JEF exports of [`ARTWORK`] apart from their thread numbers, listed as
/// [`SYNTHETIC_THREADS`] is.
const ARTWORK_THREADS: &str = "
0527952da68e1a56eca25b22f95c71026a3f3fa1f2fa411914f75849359d31d7  bullseye-twemoji.pes  29 32 5 33 20 36
8ccff94fb7d032caff8f54f490994366784da7bb0b985bc8256b2e2ea9a7d2ea  bullseye-twemoji.pec  29 32 5 33 20 36
45c126406bec505658e0434cac29838aa66a8121c437da355c2b14d8a5dcc542  bullseye-twemoji.jef  23 28 33 72 1 11
ad00a31720a9a0f36ee338f08248895ad5cf496355711048596cf729b7b03c51  fox-twemoji.clipart.pes  52 12 20 51
d4c9958d4b8a584620a0d64bffef673ee6b28a6b44c7c6a0e054632ce480d735  fox-twemoji.clipart.pec  52 12 20 51
436a5dd5417b7aa497eb5e43e06fa012ed3ab7eddfad2d10ababe15a3a76f2fb  fox-twemoji.clipart.jef  71 13 1 42
c42660ea6f3f62c28e16e77b32c262173236f89141d2a0439b2f00c6ec393291  heart-suit-twemoji.clipart.pes  51
18d868d546bf36f1d8870299a63c2404ed07af8b2a96f179d58cf057443be487  heart-suit-twemoji.clipart.pec  51
3a582db45cd3c3908ee333d943b2c2a509f997b21fc7e26752a8b06313f76e05  heart-suit-twemoji.clipart.jef  42
c0afa92e4c6966ab680077fbc22d17945d80437c0992d5ac34afb828a508215c  joystick-twemoji.pes  16 48 23 39
04b87ae0e91fc98ca0d3c5fc5d9167be0aaf426f2c135d6841028218e0882431  joystick-twemoji.pec  16 48 23 39
f476081b2158eac9b1dbe2657420ab152052639a42ad03cec521bf37bdb295e6  joystick-twemoji.jef  21 33 50 50
27e2658254542a92fbc6fc620ca46775d8dc2449454f2e6bf31a8996d09cd5a0  pool-8-ball-twemoji.pes  23 29 23
1596c6f7db7be15b02dbc4371767c30da8f080355a91656db1bcd557fe343c7a  pool-8-ball-twemoji.pec  23 29 23
3a10d758edeab9ce9be44b30e5b251118a450410953521136421bc320b407dd7  pool-8-ball-twemoji.jef  50 23 50
0666920eb3f75fe49602c67f6d93b08acbd49ec07f9c7553375fdd7f51dfb8bb  puzzle-piece-twemoji.pes  37
8dcf84136e866c7a24177ffff44a0f15341a5c8a3fecc48eae37abe56a8239c4  puzzle-piece-twemoji.pec  37
9a222e0ddc1914f91f07e5b08f32b8e1851a4eeb99182b5cf092638e48ca8637  puzzle-piece-twemoji.jef  22
1a688cd77eb918579f06522b619aa9e6ee2b00d80438394fb33fedd0669bfe86  smiling-face-with-smiling-eyes-twemoji.clipart.pes  58 43 23
858f9b7f9db698698862334d83a5c6613efc641dd5e598e64a82365badd04ab9  smiling-face-with-smiling-eyes-twemoji.clipart.pec  58 43 23
d9352300ecee75a7018d15506102c727c8a2fd4e900727cca1fbac1f4b4c133b  smiling-face-with-smiling-eyes-twemoji.clipart.jef  73 9 1
f7e159c9fc72d844c582edee56118026975f4a3259715cc33f4edc1a9fd2a733  sos-button-fluent.clipart.pes  48 29
7c7558981d16ac1e06902ad4716e3700daa4ab6bb289d465744343dbd733c9cc  sos-button-fluent.clipart.pec  48 29
e9ca7ca008223660401fced648d91a3651d77badf02a520054a8938e247103ca  sos-button-fluent.clipart.jef  32 2
2e17b2fbd6af7c627264a1457ab89c08608f64cbec5782bd3b02d0910e28a651  video-game-twemoji.pes  23 20 39 12 37 32 5 39
b5789467912884ed0e4986aa2930526e0eed242d996ee67069b56c5464c3c492  video-game-twemoji.pec  23 20 39 12 37 32 5 39
91c8ba74cae597feccbfdd0c4d5b225ce808fb6008d1b5df34b783ee5167ac0d  video-game-twemoji.jef  50 1 50 37 22 28 33 24
6f59ff87ff9764a5db53aac26f86fe086a243fc9d6c64778cac2f3b16793ae78  watermelon-twemoji.clipart.pes  54 29 5 20
82e3b64833718cec47b8fbec16b274ec6b0c0bce82b6e803662caf29378fcb1a  watermelon-twemoji.clipart.pec  54 29 5 20
fc1c386b8c347c703e9a6947af98a0770a85b6eb23c85977dfd78bab01cee641  watermelon-twemoji.clipart.jef  6 51 42 1
223f7e65b78c7fb1f42cf4bee2a6f6cea04cab88ed74bf74bd36662416c66e60  zebra-twemoji.clipart.pes  29 20 29 39 23 20 39 20
923f39ac5003919e729f00e5c0d50c140d50689666d99566de42527c6e6bf022  zebra-twemoji.clipart.pec  29 20 29 39 23 20 39 20
2853d6536561088d04462d9cdaed53bdd8d48d8f9895f06a9155eb9a8dd55ea5  zebra-twemoji.clipart.jef  51 1 51 50 1 1 50 1
";

/// What each export of [`SYNTHETIC`] holds, read back ([`Summary`]): its name, then its fields.
const SYNTHETIC_SUMMARIES: &str = "
fills.dst  blocks 4  needle 356/133/302/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.41  cells 577
fills.tap  blocks 4  needle 356/133/302/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.41  cells 577
fills.t01  blocks 4  needle 356/133/302/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.41  cells 577
fills.exp  blocks 4  needle 356/133/302/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.41  cells 577
fills.pes  blocks 4  needle 356/133/302/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.40  cells 576
fills.pec  blocks 4  needle 356/133/302/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.40  cells 576
fills.jef  blocks 4  needle 356/133/302/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.9  longest 4.60  shortest 0.41  cells 576
fills.vp3  blocks 4  needle 356/133/302/90  jumps 2/1/1/1  trims 0/0/0/0  size 76.0x70.9  longest 4.60  shortest 0.41  cells 576
strokes.dst  blocks 6  needle 353/611/265/66/557/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.60  shortest 0.41  cells 1592
strokes.tap  blocks 6  needle 353/611/265/66/557/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.60  shortest 0.41  cells 1592
strokes.t01  blocks 6  needle 353/611/265/66/557/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.60  shortest 0.41  cells 1592
strokes.exp  blocks 6  needle 353/611/265/66/557/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.60  shortest 0.41  cells 1592
strokes.pes  blocks 6  needle 353/611/265/66/557/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.67  shortest 0.41  cells 1593
strokes.pec  blocks 6  needle 353/611/265/66/557/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.67  shortest 0.41  cells 1593
strokes.jef  blocks 6  needle 353/611/265/66/557/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.2x89.5  longest 4.60  shortest 0.50  cells 1598
strokes.vp3  blocks 6  needle 353/611/265/66/557/390  jumps 1/1/1/2/1/1  trims 0/0/0/0/0/0  size 88.2x89.5  longest 4.60  shortest 0.50  cells 1598
compound-path-with-a-hole.dst  blocks 2  needle 1019/361  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.41  cells 1069
compound-path-with-a-hole.tap  blocks 2  needle 1019/361  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.41  cells 1069
compound-path-with-a-hole.t01  blocks 2  needle 1019/361  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.41  cells 1069
compound-path-with-a-hole.exp  blocks 2  needle 1019/361  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.41  cells 1069
compound-path-with-a-hole.pes  blocks 2  needle 1019/361  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.41  cells 1069
compound-path-with-a-hole.pec  blocks 2  needle 1019/361  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.41  cells 1069
compound-path-with-a-hole.jef  blocks 2  needle 1019/361  jumps 1/4  trims 0/3  size 74.3x28.5  longest 4.67  shortest 0.40  cells 1069
compound-path-with-a-hole.vp3  blocks 2  needle 1019/361  jumps 1/4  trims 0/0  size 74.3x28.5  longest 4.67  shortest 0.40  cells 1069
clip-groups.dst  blocks 5  needle 1164/1163/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.32  cells 1192
clip-groups.tap  blocks 5  needle 1164/1163/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.32  cells 1192
clip-groups.t01  blocks 5  needle 1164/1163/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.32  cells 1192
clip-groups.exp  blocks 5  needle 1164/1163/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.32  cells 1192
clip-groups.pes  blocks 5  needle 1164/1163/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.53  shortest 0.41  cells 1193
clip-groups.pec  blocks 5  needle 1164/1163/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.53  shortest 0.41  cells 1193
clip-groups.jef  blocks 5  needle 1164/1163/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.40  cells 1193
clip-groups.vp3  blocks 5  needle 1164/1163/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/0  size 36.1x36.1  longest 4.60  shortest 0.40  cells 1193
symbols.dst  blocks 2  needle 657/437  jumps 3/3  trims 2/2  size 51.4x74.5  longest 4.67  shortest 0.42  cells 559
symbols.tap  blocks 2  needle 657/437  jumps 3/3  trims 2/2  size 51.4x74.5  longest 4.67  shortest 0.42  cells 559
symbols.t01  blocks 2  needle 657/437  jumps 3/3  trims 2/2  size 51.4x74.5  longest 4.67  shortest 0.42  cells 559
symbols.exp  blocks 2  needle 657/437  jumps 3/3  trims 2/2  size 51.4x74.5  longest 4.67  shortest 0.42  cells 559
symbols.pes  blocks 2  needle 657/437  jumps 3/3  trims 2/2  size 51.4x74.5  longest 4.60  shortest 0.42  cells 559
symbols.pec  blocks 2  needle 657/437  jumps 3/3  trims 2/2  size 51.4x74.5  longest 4.60  shortest 0.42  cells 559
symbols.jef  blocks 2  needle 657/437  jumps 3/3  trims 2/2  size 51.4x74.5  longest 4.67  shortest 0.50  cells 561
symbols.vp3  blocks 2  needle 657/437  jumps 3/3  trims 0/0  size 51.4x74.5  longest 4.67  shortest 0.50  cells 561
trim-after.dst  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.60  shortest 0.41  cells 144
trim-after.tap  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.60  shortest 0.41  cells 144
trim-after.t01  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.60  shortest 0.41  cells 144
trim-after.exp  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.60  shortest 0.41  cells 144
trim-after.pes  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.60  shortest 0.41  cells 144
trim-after.pec  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.60  shortest 0.41  cells 144
trim-after.jef  blocks 1  needle 242  jumps 4  trims 2  size 26.8x5.6  longest 4.67  shortest 0.41  cells 144
trim-after.vp3  blocks 1  needle 242  jumps 4  trims 0  size 26.8x5.6  longest 4.67  shortest 0.41  cells 144
opened-design.dst  blocks 2  needle 356/133  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.60  shortest 0.40  cells 367
opened-design.tap  blocks 2  needle 356/133  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.60  shortest 0.40  cells 367
opened-design.t01  blocks 2  needle 356/133  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.60  shortest 0.40  cells 367
opened-design.exp  blocks 2  needle 356/133  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.60  shortest 0.40  cells 367
opened-design.pes  blocks 2  needle 356/133  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.60  shortest 0.40  cells 367
opened-design.pec  blocks 2  needle 356/133  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.60  shortest 0.40  cells 367
opened-design.jef  blocks 2  needle 356/133  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.60  shortest 0.40  cells 367
opened-design.vp3  blocks 2  needle 356/133  jumps 2/1  trims 0/0  size 35.5x42.6  longest 4.60  shortest 0.40  cells 367
opened-design-clipped.dst  blocks 1  needle 122  jumps 14  trims 1  size 7.3x10.9  longest 4.53  shortest 0.41  cells 80
opened-design-clipped.tap  blocks 1  needle 122  jumps 14  trims 1  size 7.3x10.9  longest 4.53  shortest 0.41  cells 80
opened-design-clipped.t01  blocks 1  needle 122  jumps 14  trims 1  size 7.3x10.9  longest 4.53  shortest 0.41  cells 80
opened-design-clipped.exp  blocks 1  needle 122  jumps 14  trims 1  size 7.3x10.9  longest 4.53  shortest 0.41  cells 80
opened-design-clipped.pes  blocks 1  needle 122  jumps 14  trims 1  size 7.3x10.9  longest 4.53  shortest 0.41  cells 80
opened-design-clipped.pec  blocks 1  needle 122  jumps 14  trims 1  size 7.3x10.9  longest 4.53  shortest 0.41  cells 80
opened-design-clipped.jef  blocks 1  needle 122  jumps 14  trims 1  size 7.3x10.9  longest 4.53  shortest 0.41  cells 80
opened-design-clipped.vp3  blocks 1  needle 122  jumps 14  trims 0  size 7.3x10.9  longest 4.53  shortest 0.41  cells 80
opened-design-cut-at-block-ends.dst  blocks 2  needle 20/20  jumps 3/2  trims 0/1  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.tap  blocks 2  needle 20/20  jumps 3/2  trims 0/1  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.t01  blocks 2  needle 20/20  jumps 3/2  trims 0/1  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.exp  blocks 2  needle 20/20  jumps 2/2  trims 1/2  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.pes  blocks 2  needle 20/20  jumps 2/2  trims 1/2  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.pec  blocks 2  needle 20/20  jumps 2/2  trims 1/2  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.jef  blocks 2  needle 20/20  jumps 2/2  trims 1/2  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.vp3  blocks 2  needle 20/20  jumps 2/2  trims 0/0  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
mixed-stitch-style-fill.dst  blocks 6  needle 397/325/336/96/546/413  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 85.9x78.6  longest 4.67  shortest 0.36  cells 1451
mixed-stitch-style-fill.tap  blocks 6  needle 397/325/336/96/546/413  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 85.9x78.6  longest 4.67  shortest 0.36  cells 1451
mixed-stitch-style-fill.t01  blocks 6  needle 397/325/336/96/546/413  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 85.9x78.6  longest 4.67  shortest 0.36  cells 1451
mixed-stitch-style-fill.exp  blocks 6  needle 397/325/336/96/546/413  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 85.9x78.6  longest 4.67  shortest 0.36  cells 1451
mixed-stitch-style-fill.pes  blocks 6  needle 397/325/336/96/546/413  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 85.9x78.6  longest 4.67  shortest 0.36  cells 1449
mixed-stitch-style-fill.pec  blocks 6  needle 397/325/336/96/546/413  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 85.9x78.6  longest 4.67  shortest 0.36  cells 1449
mixed-stitch-style-fill.jef  blocks 6  needle 397/325/336/96/546/413  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1468
mixed-stitch-style-fill.vp3  blocks 6  needle 397/325/336/96/546/413  jumps 1/1/1/2/1/1  trims 0/0/0/0/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1468
mixed-stitch-style-satin.dst  blocks 6  needle 443/293/369/365/639/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.5  longest 3.42  shortest 0.41  cells 1488
mixed-stitch-style-satin.tap  blocks 6  needle 443/293/369/365/639/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.5  longest 3.42  shortest 0.41  cells 1488
mixed-stitch-style-satin.t01  blocks 6  needle 443/293/369/365/639/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.5  longest 3.42  shortest 0.41  cells 1488
mixed-stitch-style-satin.exp  blocks 6  needle 443/293/369/365/639/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.5  longest 3.42  shortest 0.41  cells 1488
mixed-stitch-style-satin.pes  blocks 6  needle 443/293/369/365/639/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.5  longest 3.42  shortest 0.41  cells 1492
mixed-stitch-style-satin.pec  blocks 6  needle 443/293/369/365/639/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.5  longest 3.42  shortest 0.41  cells 1492
mixed-stitch-style-satin.jef  blocks 6  needle 443/293/369/365/639/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.5  longest 3.42  shortest 0.41  cells 1495
mixed-stitch-style-satin.vp3  blocks 6  needle 443/293/369/365/639/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.5  longest 3.42  shortest 0.41  cells 1495
mixed-stitch-style-running.dst  blocks 6  needle 31/44/31/37/37/37  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 437
mixed-stitch-style-running.tap  blocks 6  needle 31/44/31/37/37/37  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 437
mixed-stitch-style-running.t01  blocks 6  needle 31/44/31/37/37/37  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 437
mixed-stitch-style-running.exp  blocks 6  needle 31/44/31/37/37/37  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 437
mixed-stitch-style-running.pes  blocks 6  needle 31/44/31/37/37/37  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 440
mixed-stitch-style-running.pec  blocks 6  needle 31/44/31/37/37/37  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 440
mixed-stitch-style-running.jef  blocks 6  needle 31/44/31/37/37/37  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.6x77.6  longest 3.00  shortest 0.98  cells 436
mixed-stitch-style-running.vp3  blocks 6  needle 31/44/31/37/37/37  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.6x77.6  longest 3.00  shortest 0.98  cells 436
mixed-every-option.dst  blocks 6  needle 287/233/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.50  cells 1722
mixed-every-option.tap  blocks 6  needle 287/233/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.50  cells 1722
mixed-every-option.t01  blocks 6  needle 287/233/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.50  cells 1722
mixed-every-option.exp  blocks 6  needle 287/233/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.50  cells 1722
mixed-every-option.pes  blocks 6  needle 287/233/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.50  cells 1723
mixed-every-option.pec  blocks 6  needle 287/233/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.50  cells 1723
mixed-every-option.jef  blocks 6  needle 287/233/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 89.0x80.4  longest 5.28  shortest 0.50  cells 1735
mixed-every-option.vp3  blocks 6  needle 287/233/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 89.0x80.4  longest 5.28  shortest 0.50  cells 1735
mixed-shrunk.dst  blocks 6  needle 377/299/368/37/522/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.41  cells 1358
mixed-shrunk.tap  blocks 6  needle 377/299/368/37/522/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.41  cells 1358
mixed-shrunk.t01  blocks 6  needle 377/299/368/37/522/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.41  cells 1358
mixed-shrunk.exp  blocks 6  needle 377/299/368/37/522/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.41  cells 1358
mixed-shrunk.pes  blocks 6  needle 377/299/368/37/522/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.41  cells 1357
mixed-shrunk.pec  blocks 6  needle 377/299/368/37/522/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.41  cells 1357
mixed-shrunk.jef  blocks 6  needle 377/299/368/37/522/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.36  cells 1358
mixed-shrunk.vp3  blocks 6  needle 377/299/368/37/522/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.36  cells 1358
";

/// What each export of [`ARTWORK`] holds, read back, listed as [`SYNTHETIC_SUMMARIES`] is.
const ARTWORK_SUMMARIES: &str = "
bullseye-twemoji.dst  blocks 6  needle 1553/988/546/232/292/159  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.0x45.0  longest 8.47  shortest 0.32  cells 1694
bullseye-twemoji.tap  blocks 6  needle 1553/988/546/232/292/159  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.0x45.0  longest 8.47  shortest 0.32  cells 1694
bullseye-twemoji.t01  blocks 6  needle 1553/988/546/232/292/159  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.0x45.0  longest 8.47  shortest 0.32  cells 1694
bullseye-twemoji.exp  blocks 6  needle 1553/988/546/232/292/159  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.0x45.0  longest 8.47  shortest 0.32  cells 1694
bullseye-twemoji.pes  blocks 6  needle 1553/988/546/232/292/159  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.0x45.0  longest 8.42  shortest 0.32  cells 1696
bullseye-twemoji.pec  blocks 6  needle 1553/988/546/232/292/159  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.0x45.0  longest 8.42  shortest 0.32  cells 1696
bullseye-twemoji.jef  blocks 6  needle 1553/988/546/232/292/159  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.0x45.0  longest 8.45  shortest 0.32  cells 1693
bullseye-twemoji.vp3  blocks 6  needle 1553/988/546/232/292/159  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.0x45.0  longest 8.45  shortest 0.32  cells 1693
fox-twemoji.clipart.dst  blocks 4  needle 1391/461/144/247  jumps 1/2/3/2  trims 0/0/2/1  size 45.1x40.6  longest 10.03  shortest 0.32  cells 1321
fox-twemoji.clipart.tap  blocks 4  needle 1391/461/144/247  jumps 1/2/3/2  trims 0/0/2/1  size 45.1x40.6  longest 10.03  shortest 0.32  cells 1321
fox-twemoji.clipart.t01  blocks 4  needle 1391/461/144/247  jumps 1/2/3/2  trims 0/0/2/1  size 45.1x40.6  longest 10.03  shortest 0.32  cells 1321
fox-twemoji.clipart.exp  blocks 4  needle 1391/461/144/247  jumps 1/2/3/2  trims 0/0/2/1  size 45.1x40.6  longest 10.03  shortest 0.32  cells 1321
fox-twemoji.clipart.pes  blocks 4  needle 1391/461/144/247  jumps 1/2/3/2  trims 0/0/2/1  size 45.1x40.6  longest 10.03  shortest 0.32  cells 1323
fox-twemoji.clipart.pec  blocks 4  needle 1391/461/144/247  jumps 1/2/3/2  trims 0/0/2/1  size 45.1x40.6  longest 10.03  shortest 0.32  cells 1323
fox-twemoji.clipart.jef  blocks 4  needle 1391/461/144/247  jumps 1/2/3/2  trims 0/0/2/1  size 45.1x40.6  longest 9.99  shortest 0.32  cells 1322
fox-twemoji.clipart.vp3  blocks 4  needle 1391/461/144/247  jumps 1/2/3/2  trims 0/0/0/0  size 45.1x40.6  longest 9.99  shortest 0.32  cells 1322
heart-suit-twemoji.clipart.dst  blocks 1  needle 1349  jumps 1  trims 0  size 45.0x42.0  longest 4.60  shortest 0.41  cells 1351
heart-suit-twemoji.clipart.tap  blocks 1  needle 1349  jumps 1  trims 0  size 45.0x42.0  longest 4.60  shortest 0.41  cells 1351
heart-suit-twemoji.clipart.t01  blocks 1  needle 1349  jumps 1  trims 0  size 45.0x42.0  longest 4.60  shortest 0.41  cells 1351
heart-suit-twemoji.clipart.exp  blocks 1  needle 1349  jumps 1  trims 0  size 45.0x42.0  longest 4.60  shortest 0.41  cells 1351
heart-suit-twemoji.clipart.pes  blocks 1  needle 1349  jumps 1  trims 0  size 45.0x42.0  longest 4.60  shortest 0.32  cells 1351
heart-suit-twemoji.clipart.pec  blocks 1  needle 1349  jumps 1  trims 0  size 45.0x42.0  longest 4.60  shortest 0.32  cells 1351
heart-suit-twemoji.clipart.jef  blocks 1  needle 1349  jumps 1  trims 0  size 45.0x42.0  longest 4.67  shortest 0.32  cells 1356
heart-suit-twemoji.clipart.vp3  blocks 1  needle 1349  jumps 1  trims 0  size 45.0x42.0  longest 4.67  shortest 0.32  cells 1356
joystick-twemoji.dst  blocks 4  needle 154/245/1020/227  jumps 1/2/4/1  trims 0/1/2/0  size 42.4x45.1  longest 5.70  shortest 0.41  cells 980
joystick-twemoji.tap  blocks 4  needle 154/245/1020/227  jumps 1/2/4/1  trims 0/1/2/0  size 42.4x45.1  longest 5.70  shortest 0.41  cells 980
joystick-twemoji.t01  blocks 4  needle 154/245/1020/227  jumps 1/2/4/1  trims 0/1/2/0  size 42.4x45.1  longest 5.70  shortest 0.41  cells 980
joystick-twemoji.exp  blocks 4  needle 154/245/1020/227  jumps 1/2/4/1  trims 0/1/2/0  size 42.4x45.1  longest 5.70  shortest 0.41  cells 980
joystick-twemoji.pes  blocks 4  needle 154/245/1020/227  jumps 1/2/4/1  trims 0/1/2/0  size 42.4x45.1  longest 5.70  shortest 0.32  cells 981
joystick-twemoji.pec  blocks 4  needle 154/245/1020/227  jumps 1/2/4/1  trims 0/1/2/0  size 42.4x45.1  longest 5.70  shortest 0.32  cells 981
joystick-twemoji.jef  blocks 4  needle 154/245/1020/227  jumps 1/2/4/1  trims 0/1/2/0  size 42.5x45.1  longest 5.70  shortest 0.41  cells 981
joystick-twemoji.vp3  blocks 4  needle 154/245/1020/227  jumps 1/2/4/1  trims 0/0/0/0  size 42.5x45.1  longest 5.70  shortest 0.41  cells 981
pool-8-ball-twemoji.dst  blocks 3  needle 1613/475/295  jumps 1/1/1  trims 0/0/0  size 46.0x46.0  longest 4.75  shortest 0.32  cells 1734
pool-8-ball-twemoji.tap  blocks 3  needle 1613/475/295  jumps 1/1/1  trims 0/0/0  size 46.0x46.0  longest 4.75  shortest 0.32  cells 1734
pool-8-ball-twemoji.t01  blocks 3  needle 1613/475/295  jumps 1/1/1  trims 0/0/0  size 46.0x46.0  longest 4.75  shortest 0.32  cells 1734
pool-8-ball-twemoji.exp  blocks 3  needle 1613/475/295  jumps 1/1/1  trims 0/0/0  size 46.0x46.0  longest 4.75  shortest 0.32  cells 1734
pool-8-ball-twemoji.pes  blocks 3  needle 1613/475/295  jumps 1/1/1  trims 0/0/0  size 46.0x46.0  longest 4.75  shortest 0.32  cells 1733
pool-8-ball-twemoji.pec  blocks 3  needle 1613/475/295  jumps 1/1/1  trims 0/0/0  size 46.0x46.0  longest 4.75  shortest 0.32  cells 1733
pool-8-ball-twemoji.jef  blocks 3  needle 1613/475/295  jumps 1/1/1  trims 0/0/0  size 46.0x46.0  longest 4.69  shortest 0.32  cells 1737
pool-8-ball-twemoji.vp3  blocks 3  needle 1613/475/295  jumps 1/1/1  trims 0/0/0  size 46.0x46.0  longest 4.69  shortest 0.32  cells 1737
puzzle-piece-twemoji.dst  blocks 1  needle 1368  jumps 1  trims 0  size 45.1x45.1  longest 4.60  shortest 0.32  cells 1204
puzzle-piece-twemoji.tap  blocks 1  needle 1368  jumps 1  trims 0  size 45.1x45.1  longest 4.60  shortest 0.32  cells 1204
puzzle-piece-twemoji.t01  blocks 1  needle 1368  jumps 1  trims 0  size 45.1x45.1  longest 4.60  shortest 0.32  cells 1204
puzzle-piece-twemoji.exp  blocks 1  needle 1368  jumps 1  trims 0  size 45.1x45.1  longest 4.60  shortest 0.32  cells 1204
puzzle-piece-twemoji.pes  blocks 1  needle 1368  jumps 1  trims 0  size 45.1x45.1  longest 4.60  shortest 0.36  cells 1204
puzzle-piece-twemoji.pec  blocks 1  needle 1368  jumps 1  trims 0  size 45.1x45.1  longest 4.60  shortest 0.36  cells 1204
puzzle-piece-twemoji.jef  blocks 1  needle 1368  jumps 1  trims 0  size 45.1x45.0  longest 4.61  shortest 0.32  cells 1207
puzzle-piece-twemoji.vp3  blocks 1  needle 1368  jumps 1  trims 0  size 45.1x45.0  longest 4.61  shortest 0.32  cells 1207
smiling-face-with-smiling-eyes-twemoji.clipart.dst  blocks 3  needle 1890/365/384  jumps 1/2/3  trims 0/1/0  size 47.5x47.3  longest 5.92  shortest 0.32  cells 1834
smiling-face-with-smiling-eyes-twemoji.clipart.tap  blocks 3  needle 1890/365/384  jumps 1/2/3  trims 0/1/0  size 47.5x47.3  longest 5.92  shortest 0.32  cells 1834
smiling-face-with-smiling-eyes-twemoji.clipart.t01  blocks 3  needle 1890/365/384  jumps 1/2/3  trims 0/1/0  size 47.5x47.3  longest 5.92  shortest 0.32  cells 1834
smiling-face-with-smiling-eyes-twemoji.clipart.exp  blocks 3  needle 1890/365/384  jumps 1/2/3  trims 0/1/0  size 47.5x47.3  longest 5.92  shortest 0.32  cells 1834
smiling-face-with-smiling-eyes-twemoji.clipart.pes  blocks 3  needle 1890/365/384  jumps 1/2/3  trims 0/1/0  size 47.5x47.3  longest 5.92  shortest 0.36  cells 1833
smiling-face-with-smiling-eyes-twemoji.clipart.pec  blocks 3  needle 1890/365/384  jumps 1/2/3  trims 0/1/0  size 47.5x47.3  longest 5.92  shortest 0.36  cells 1833
smiling-face-with-smiling-eyes-twemoji.clipart.jef  blocks 3  needle 1890/365/384  jumps 1/2/3  trims 0/1/0  size 47.5x47.3  longest 6.02  shortest 0.40  cells 1833
smiling-face-with-smiling-eyes-twemoji.clipart.vp3  blocks 3  needle 1890/365/384  jumps 1/2/3  trims 0/0/0  size 47.5x47.3  longest 6.02  shortest 0.40  cells 1833
sos-button-fluent.clipart.dst  blocks 2  needle 1961/764  jumps 1/3  trims 0/1  size 45.0x45.0  longest 4.60  shortest 0.41  cells 2069
sos-button-fluent.clipart.tap  blocks 2  needle 1961/764  jumps 1/3  trims 0/1  size 45.0x45.0  longest 4.60  shortest 0.41  cells 2069
sos-button-fluent.clipart.t01  blocks 2  needle 1961/764  jumps 1/3  trims 0/1  size 45.0x45.0  longest 4.60  shortest 0.41  cells 2069
sos-button-fluent.clipart.exp  blocks 2  needle 1961/764  jumps 1/3  trims 0/1  size 45.0x45.0  longest 4.60  shortest 0.41  cells 2069
sos-button-fluent.clipart.pes  blocks 2  needle 1961/764  jumps 1/3  trims 0/1  size 45.0x45.0  longest 4.60  shortest 0.41  cells 2069
sos-button-fluent.clipart.pec  blocks 2  needle 1961/764  jumps 1/3  trims 0/1  size 45.0x45.0  longest 4.60  shortest 0.41  cells 2069
sos-button-fluent.clipart.jef  blocks 2  needle 1961/764  jumps 1/3  trims 0/1  size 45.0x45.0  longest 4.60  shortest 0.41  cells 2069
sos-button-fluent.clipart.vp3  blocks 2  needle 1961/764  jumps 1/3  trims 0/0  size 45.0x45.0  longest 4.60  shortest 0.41  cells 2069
video-game-twemoji.dst  blocks 8  needle 582/1181/140/31/30/31/29/143  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.0x31.9  longest 4.67  shortest 0.32  cells 1203
video-game-twemoji.tap  blocks 8  needle 582/1181/140/31/30/31/29/143  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.0x31.9  longest 4.67  shortest 0.32  cells 1203
video-game-twemoji.t01  blocks 8  needle 582/1181/140/31/30/31/29/143  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.0x31.9  longest 4.67  shortest 0.32  cells 1203
video-game-twemoji.exp  blocks 8  needle 582/1181/140/31/30/31/29/143  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.0x31.9  longest 4.67  shortest 0.32  cells 1203
video-game-twemoji.pes  blocks 8  needle 582/1181/140/31/30/31/29/143  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.0x31.9  longest 4.67  shortest 0.36  cells 1203
video-game-twemoji.pec  blocks 8  needle 582/1181/140/31/30/31/29/143  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.0x31.9  longest 4.67  shortest 0.36  cells 1203
video-game-twemoji.jef  blocks 8  needle 582/1181/140/31/30/31/29/143  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.0x32.0  longest 4.60  shortest 0.32  cells 1205
video-game-twemoji.vp3  blocks 8  needle 582/1181/140/31/30/31/29/143  jumps 4/3/2/1/1/1/1/4  trims 0/0/0/0/0/0/0/0  size 45.0x32.0  longest 4.60  shortest 0.32  cells 1205
watermelon-twemoji.clipart.dst  blocks 4  needle 408/363/590/148  jumps 1/1/1/8  trims 0/0/0/0  size 45.8x31.9  longest 4.46  shortest 0.32  cells 949
watermelon-twemoji.clipart.tap  blocks 4  needle 408/363/590/148  jumps 1/1/1/8  trims 0/0/0/0  size 45.8x31.9  longest 4.46  shortest 0.32  cells 949
watermelon-twemoji.clipart.t01  blocks 4  needle 408/363/590/148  jumps 1/1/1/8  trims 0/0/0/0  size 45.8x31.9  longest 4.46  shortest 0.32  cells 949
watermelon-twemoji.clipart.exp  blocks 4  needle 408/363/590/148  jumps 1/1/1/8  trims 0/0/0/0  size 45.8x31.9  longest 4.46  shortest 0.32  cells 949
watermelon-twemoji.clipart.pes  blocks 4  needle 408/363/590/148  jumps 1/1/1/8  trims 0/0/0/0  size 45.8x31.9  longest 4.53  shortest 0.32  cells 948
watermelon-twemoji.clipart.pec  blocks 4  needle 408/363/590/148  jumps 1/1/1/8  trims 0/0/0/0  size 45.8x31.9  longest 4.53  shortest 0.32  cells 948
watermelon-twemoji.clipart.jef  blocks 4  needle 408/363/590/148  jumps 1/1/1/8  trims 0/0/0/0  size 45.8x31.9  longest 4.46  shortest 0.32  cells 950
watermelon-twemoji.clipart.vp3  blocks 4  needle 408/363/590/148  jumps 1/1/1/8  trims 0/0/0/0  size 45.8x31.9  longest 4.46  shortest 0.32  cells 950
zebra-twemoji.clipart.dst  blocks 8  needle 1561/323/269/43/24/566/30/764  jumps 3/2/2/1/1/3/1/9  trims 2/0/1/0/0/1/0/3  size 45.1x45.6  longest 6.94  shortest 0.32  cells 1566
zebra-twemoji.clipart.tap  blocks 8  needle 1561/323/269/43/24/566/30/764  jumps 3/2/2/1/1/3/1/9  trims 2/0/1/0/0/1/0/3  size 45.1x45.6  longest 6.94  shortest 0.32  cells 1566
zebra-twemoji.clipart.t01  blocks 8  needle 1561/323/269/43/24/566/30/764  jumps 3/2/2/1/1/3/1/9  trims 2/0/1/0/0/1/0/3  size 45.1x45.6  longest 6.94  shortest 0.32  cells 1566
zebra-twemoji.clipart.exp  blocks 8  needle 1561/323/269/43/24/566/30/764  jumps 3/2/2/1/1/3/1/9  trims 2/0/1/0/0/1/0/3  size 45.1x45.6  longest 6.94  shortest 0.32  cells 1566
zebra-twemoji.clipart.pes  blocks 8  needle 1561/323/269/43/24/566/30/764  jumps 3/2/2/1/1/3/1/9  trims 2/0/1/0/0/1/0/3  size 45.1x45.6  longest 6.98  shortest 0.36  cells 1569
zebra-twemoji.clipart.pec  blocks 8  needle 1561/323/269/43/24/566/30/764  jumps 3/2/2/1/1/3/1/9  trims 2/0/1/0/0/1/0/3  size 45.1x45.6  longest 6.98  shortest 0.36  cells 1569
zebra-twemoji.clipart.jef  blocks 8  needle 1561/323/269/43/24/566/30/764  jumps 3/2/2/1/1/3/1/9  trims 2/0/1/0/0/1/0/3  size 45.1x45.6  longest 6.94  shortest 0.32  cells 1567
zebra-twemoji.clipart.vp3  blocks 8  needle 1561/323/269/43/24/566/30/764  jumps 3/2/2/1/1/3/1/9  trims 0/0/0/0/0/0/0/0  size 45.1x45.6  longest 6.94  shortest 0.32  cells 1567
";

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
/// Where a PEC file's PEC header starts: after its magic `#PEC0001`.
const PEC_HEADER_AT: usize = 8;
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
        let plan = planned(&mut s, &options);
        for format in Format::ALL {
            let bytes = export(&mut s, format, &options).unwrap_or_else(|e| panic!("{name} as {}: {e}", format.id()));
            let pinned = Pinned::new(format!("{name}.{}", format.extension()), format, &bytes);
            plan.holds(&pinned, format);
            now.push(pinned);
        }
    }
    check(SYNTHETIC, SYNTHETIC_THREADS, SYNTHETIC_SUMMARIES, &now);
}

/// What a session's design plans (`embroidery.stats`): each block's needle points, and its longest
/// needle-down stitch (millimetres).
struct Planned {
    needle: Vec<usize>,
    longest: f64,
}

/// The plan of the session's design exported with `options`.
fn planned(s: &mut Session, options: &Value) -> Planned {
    let stats = run(s, "embroidery.stats", options.clone());
    let needle = stats["perBlock"].as_array().unwrap().iter().map(|b| b["stitches"].as_u64().unwrap() as usize).collect();
    Planned { needle, longest: stats["longestStitch"].as_f64().unwrap_or(0.0) }
}

impl Planned {
    /// An export read back has the plan's needle points, block by block, when no planned stitch is
    /// longer than one of its format's records holds (a longer one is written as several).
    fn holds(&self, pinned: &Pinned, format: Format) {
        if self.longest <= format.max_step() {
            assert_eq!(pinned.summary.needle, self.needle, "{}: the needle points read back are not the plan's", pinned.name);
        }
    }
}

/// One export as the listings pin it.
struct Pinned {
    /// The document's name and the format's extension.
    name: String,
    /// The hash of its bytes.
    hash: String,
    /// For a format that numbers its threads (PES, PEC, JEF): the hash of its bytes with the thread
    /// numbers blanked, and the numbers ([`threads_apart`]).
    threads: Option<(String, String)>,
    /// What it holds, read back by our reader of its format ([`Summary`]).
    summary: Summary,
}

impl Pinned {
    /// `bytes`, our writer's export of `name` as `format`, pinned: they must read back with our
    /// reader of the format, or the export is no file at all.
    fn new(name: String, format: Format, bytes: &[u8]) -> Self {
        let threads = threads_apart(format, bytes)
            .map(|(blanked, numbers)| (sha256::hex(&blanked), numbers.iter().map(u32::to_string).collect::<Vec<_>>().join(" ")));
        let read = format.read(bytes).unwrap_or_else(|e| panic!("{name}: our own reader can't read it: {e}"));
        Self { name, hash: sha256::hex(bytes), threads, summary: Summary::of(&read) }
    }
}

/// The side of the squares [`Summary::cells`] counts (millimetres): about a thread's width and a
/// fill row's spacing apart, so that a region of stitching sewn elsewhere, or left out, moves the
/// count.
const CELL: f64 = 1.0;
/// How finely a stitch is walked to find the squares it crosses (millimetres): a quarter of one.
const CELL_STEP: f64 = CELL / 4.0;
/// Hundredths of a millimetre, the unit the summary gives stitch lengths in.
const HUNDREDTHS_PER_MM: f64 = 100.0;
/// How far a summary's measures may move on another platform, whose maths library may round a
/// needle point to the next 0.1 mm (millimetres): one grid step at each end of a stitch.
const PLATFORM_LENGTH: f64 = 0.2;
/// A hair beyond a decimal the listing rounds to, so that a value it shows rounded compares equal.
const LISTED_ROUNDING: f64 = 1e-9;
/// How many of the squares stitched may differ on another platform, as a share of them: a needle
/// point moved by 0.1 mm moves a stitch into the next square at most.
const PLATFORM_CELLS: f64 = 0.02;

/// What an export holds, read back by our reader of its format: each block's needle points, jumps
/// and trims, the design's size, its longest and shortest needle-down stitch, and how many 1 mm
/// squares its stitches cross. Pinned beside each hash, so that a change reads field by field
/// (which block gained a trim, how far the design grew), and on another platform, where the hashes
/// may differ in the last 0.1 mm, the summaries are compared within that.
#[derive(Clone, Debug, PartialEq)]
struct Summary {
    needle: Vec<usize>,
    jumps: Vec<usize>,
    trims: Vec<usize>,
    /// Width and height, tenths of a millimetre (the readers put the design's corner at the origin).
    size: (i64, i64),
    /// Hundredths of a millimetre.
    longest: i64,
    shortest: i64,
    cells: usize,
}

impl Summary {
    fn of(p: &Pattern) -> Self {
        let count = |b: &vectorcraft_stitch::Block, c: Command| b.stitches.iter().filter(|s| s.command == c).count();
        let size = p.bounds().map_or((0, 0), |b| (quantize(b.width()).into(), quantize(b.height()).into()));
        let mut lengths: Vec<f64> = Vec::new();
        let mut cells: BTreeSet<(i64, i64)> = BTreeSet::new();
        for b in &p.blocks {
            for w in b.stitches.windows(2) {
                let [a, z] = w else { continue };
                if a.command != Command::Stitch || z.command != Command::Stitch {
                    continue;
                }
                let length = a.distance(*z);
                lengths.push(length);
                let steps = (length / CELL_STEP).ceil().max(1.0) as usize;
                for k in 0..=steps {
                    let q = a.point().lerp(z.point(), k as f64 / steps as f64);
                    cells.insert(((q.x / CELL).floor() as i64, (q.y / CELL).floor() as i64));
                }
            }
        }
        let hundredths = |v: f64| (v * HUNDREDTHS_PER_MM).round() as i64;
        let longest = lengths.iter().copied().fold(0.0, f64::max);
        let shortest = lengths.iter().copied().filter(|l| *l > 0.0).fold(f64::INFINITY, f64::min);
        Self {
            needle: p.blocks.iter().map(|b| count(b, Command::Stitch)).collect(),
            jumps: p.blocks.iter().map(|b| count(b, Command::Jump)).collect(),
            trims: p.blocks.iter().map(|b| count(b, Command::Trim)).collect(),
            size,
            longest: hundredths(longest),
            shortest: if shortest.is_finite() { hundredths(shortest) } else { 0 },
            cells: cells.len(),
        }
    }

    /// Its fields as the listing writes them, each a name and a value.
    fn fields(&self) -> Vec<(&'static str, String)> {
        let list = |v: &[usize]| v.iter().map(usize::to_string).collect::<Vec<_>>().join("/");
        let mm = |v: i64, per: f64| format!("{:.prec$}", v as f64 / per, prec = if per > 10.0 { 2 } else { 1 });
        vec![
            ("blocks", self.needle.len().to_string()),
            ("needle", list(&self.needle)),
            ("jumps", list(&self.jumps)),
            ("trims", list(&self.trims)),
            ("size", format!("{}x{}", mm(self.size.0, 10.0), mm(self.size.1, 10.0))),
            ("longest", mm(self.longest, HUNDREDTHS_PER_MM)),
            ("shortest", mm(self.shortest, HUNDREDTHS_PER_MM)),
            ("cells", self.cells.to_string()),
        ]
    }

    /// Its line in the listing, after the export's name.
    fn line(&self) -> String {
        self.fields().into_iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join("  ")
    }

    /// The fields that differ from the `recorded` line, as `name was → now`.
    fn diff(&self, recorded: &str) -> Vec<String> {
        let was: Vec<(&str, &str)> = recorded.split("  ").filter_map(|f| f.split_once(' ')).collect();
        self.fields()
            .into_iter()
            .filter_map(|(k, v)| {
                let old = was.iter().find(|(name, _)| *name == k).map_or("(none)", |(_, old)| *old);
                (old != v).then(|| format!("{k} {old} → {v}"))
            })
            .collect()
    }

    /// Whether it matches the `recorded` line within what another platform's maths library may move
    /// (counts exactly; lengths and the size within [`PLATFORM_LENGTH`]; the squares within
    /// [`PLATFORM_CELLS`]).
    fn close_to(&self, recorded: &str) -> bool {
        let was: Vec<(&str, &str)> = recorded.split("  ").filter_map(|f| f.split_once(' ')).collect();
        let get = |k: &str| was.iter().find(|(name, _)| *name == k).map(|(_, v)| *v).unwrap_or("");
        let near = |a: f64, b: &str| b.parse::<f64>().is_ok_and(|b| (a - b).abs() <= PLATFORM_LENGTH + LISTED_ROUNDING);
        let fields = self.fields();
        let now = |k: &str| fields.iter().find(|(name, _)| *name == k).map(|(_, v)| v.clone()).unwrap_or_default();
        let size_ok = get("size").split_once('x').is_some_and(|(w, h)| near(self.size.0 as f64 / 10.0, w) && near(self.size.1 as f64 / 10.0, h));
        let cells_ok = get("cells").parse::<f64>().is_ok_and(|c| (self.cells as f64 - c).abs() <= PLATFORM_CELLS * c.max(1.0));
        ["blocks", "needle", "jumps", "trims"].iter().all(|k| get(k) == now(k))
            && size_ok
            && near(self.longest as f64 / HUNDREDTHS_PER_MM, get("longest"))
            && near(self.shortest as f64 / HUNDREDTHS_PER_MM, get("shortest"))
            && cells_ok
    }
}

/// `bytes`, as our writer of `format` wrote them, with their thread numbers set to 0, and the
/// numbers, in sewing order: the PEC header's palette indices in a PES or PEC file, the header's
/// palette indices in a JEF file. `None` for a format that numbers no threads.
fn threads_apart(format: Format, bytes: &[u8]) -> Option<(Vec<u8>, Vec<u32>)> {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + WORD].try_into().unwrap()) as usize;
    let (first, count, width) = match format {
        Format::Pes | Format::Pec => {
            let header = if format == Format::Pec { PEC_HEADER_AT } else { word(PEC_POSITION) };
            let count = match bytes[header + PEC_COLOR_COUNT] {
                NO_COLORS => 0,
                c => usize::from(c) + 1,
            };
            (header + PEC_COLOR_COUNT + 1, count, 1)
        }
        Format::Jef => (JEF_THREADS, word(JEF_COLOR_COUNT), WORD),
        // VP3 writes each block's colour itself, not a number: its whole hash pins it.
        Format::Dst | Format::Tap | Format::T01 | Format::Exp | Format::Vp3 => return None,
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
        // At the person's size.
        let mut s = artwork(&dir, art, theirs);
        let plan = planned(&mut s, &json!({}));
        let stem = art.trim_end_matches(".svg");
        for format in Format::ALL {
            let bytes = export(&mut s, format, &json!({})).unwrap_or_else(|e| panic!("{art} as {}: {e}", format.id()));
            let pinned = Pinned::new(format!("{stem}.{}", format.extension()), format, &bytes);
            plan.holds(&pinned, format);
            now.push(pinned);
        }
    }
    check(ARTWORK, ARTWORK_THREADS, ARTWORK_SUMMARIES, &now);
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

/// Every export's hash is the one `listing` records, its summary the one `summaries` records, and
/// every PES and JEF export's bytes apart from its thread numbers, and the numbers, the ones
/// `threads` records; the listings have no other. Otherwise the failure names each change, field by
/// field for the summaries, says whether an export changed only in its thread numbers, and prints
/// the listings as they now stand.
///
/// On another platform than the listings were recorded on ([`RECORDED_ON`]), a hash may differ with
/// no change to the code (its maths library may round a needle point to the next 0.1 mm): there an
/// export passes when its summary is within that of the recorded one ([`Summary::close_to`]).
fn check(listing: &str, threads: &str, summaries: &str, now: &[Pinned]) {
    let recorded = lines(listing);
    let recorded_threads = lines(threads);
    let recorded_summaries: Vec<(&str, &str)> = summaries.lines().map(str::trim).filter_map(|l| l.split_once("  ")).collect();
    fn find<'a>(lines: &[Vec<&'a str>], name: &str) -> Option<Vec<&'a str>> {
        lines.iter().find(|l| l.get(1) == Some(&name)).cloned()
    }
    let here = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let recorded_here = here == RECORDED_ON;
    let mut changed: Vec<String> = Vec::new();
    for p in now {
        let mut notes: Vec<String> = Vec::new();
        let summary = recorded_summaries.iter().find(|(name, _)| *name == p.name).map(|(_, line)| *line);
        let within = summary.is_some_and(|line| p.summary.close_to(line));
        match summary {
            Some(line) if line == p.summary.line() => {}
            Some(_) if within && !recorded_here => {}
            Some(line) => notes.push(format!("summary: {}", p.summary.diff(line).join(", "))),
            None => notes.push(format!("summary not recorded → {}", p.summary.line())),
        }
        // Elsewhere, an export whose summary is within the recorded one's may differ in its bytes.
        let bytes_matter = recorded_here || !within;
        match find(&recorded, &p.name) {
            Some(l) if l[0] == p.hash => {}
            Some(_) if !bytes_matter => {}
            Some(l) => notes.push(format!("{} → {}", l[0], p.hash)),
            None => notes.push(format!("not recorded → {}", p.hash)),
        }
        if let Some((blanked, numbers)) = &p.threads {
            match find(&recorded_threads, &p.name) {
                Some(l) if l[0] != blanked && bytes_matter => notes.push(format!("more than its thread numbers: without them, {} → {blanked}", l[0])),
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
    let summaries: String = now.iter().map(|p| format!("{}  {}\n", p.name, p.summary.line())).collect();
    let platform = if recorded_here {
        String::new()
    } else {
        format!(
            "\n\nThe listing was recorded on {RECORDED_ON} and this is {here}: exports whose summaries are within the recorded ones passed; the ones listed above moved more than this platform's maths library can explain."
        )
    };
    assert!(
        changed.is_empty(),
        "{} exports changed:\n{}{platform}\n\nThe listing as it now stands:\n{listing}\nApart from the thread numbers:\n{threads}\nSummaries:\n{summaries}",
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
