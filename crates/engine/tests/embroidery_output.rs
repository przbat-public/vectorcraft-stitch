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
9686ea8f8783afd00e48069c1e0bba911cd738af28ec24f6470d007c5d4f9b77  fills.dst
2f1feb70c2a7404330f2bdd52a95bbab9a9fd31dee0e358a50a64006f6dd0927  fills.tap
2f1feb70c2a7404330f2bdd52a95bbab9a9fd31dee0e358a50a64006f6dd0927  fills.t01
48e479fed9c97885efa0cb5a23cedd1562f21c8ea00ce3da234d9e218614ee5e  fills.exp
acb451597087571f8cc640ac4b6699cfcdc1c626a1527aa536e36c9d7259192b  fills.pes
a91ee32b25bc6f292e85a91ef3d5cbcb93072a8430b2ef7fa35c1368234f23fb  fills.pec
0ad3c32be994471ec3fd17c0379f9c10d2f0f916cfe81e6ebf2487e3005b84af  fills.jef
e529a72ef95ab3f7b9247865046a8cc78b72258ed810833e2246a8ad337c2cb8  fills.vp3
656953287da199ddadcb9b773f2e4bce5fe8ab0a9a8166d67b1c5355bb8656c9  strokes.dst
0f1b8d675b4fcd377f2b811344397d2afab7c5cc04e6ebf3579600a1de387707  strokes.tap
0f1b8d675b4fcd377f2b811344397d2afab7c5cc04e6ebf3579600a1de387707  strokes.t01
4dc8987b8419f8a675631f659bef658a852429ab2608d4c0652a9ab3202dd3a7  strokes.exp
bd2135fbc92e93550dc90f1cc0d2730470c6bc4c648f1d2764cff49d08921b08  strokes.pes
c18808517767dbfe59d6461672884d0472b45652a453c44677ccb77cdca4d478  strokes.pec
782c9e20ccf857cf4814c7b20e68adccf82057964f037adbd45c9f1a4e3cbf40  strokes.jef
80624e645c12a9257700bb610543c70650cc8b5be2505f41d0f092a47a0b20f0  strokes.vp3
7d47c71b1c9299e0f379061aa69b827df96124e597c43d02edd9118f69c9366b  compound-path-with-a-hole.dst
f5b1fe51e7ddb483653481879e565d11db1ad4ea577973300254e0371ca726cc  compound-path-with-a-hole.tap
f5b1fe51e7ddb483653481879e565d11db1ad4ea577973300254e0371ca726cc  compound-path-with-a-hole.t01
6eeb8fc77450233e9b598274189424b8bcb6cd8565a6a1669ab50f90a87cf7d1  compound-path-with-a-hole.exp
a1ce319a49eb06f97369df986f15cd025a483a921e07d31c5144bb3f10e51c97  compound-path-with-a-hole.pes
d64fb69a5eb8e7ffc4e92e62168a60daab240b7a626a28413cd617a2552ac24b  compound-path-with-a-hole.pec
1c936ef36919dc251c793409621af960fea072430bf270378c3278317339f136  compound-path-with-a-hole.jef
922b90aa9b1a2a308dc8b38a826ca60a750fb6abbd4468fba306a96fcb3e7641  compound-path-with-a-hole.vp3
21b25db1bb3c719f49531ed677b322eaea5b30bbc8d3119f5f644bdef73876f7  clip-groups.dst
da046035197a234e4b15c9b27996506dfbc2f8c6dea8c85d2a252fa33ae04444  clip-groups.tap
da046035197a234e4b15c9b27996506dfbc2f8c6dea8c85d2a252fa33ae04444  clip-groups.t01
88eeee00a5641ddaee652b3d1c3b06ca8f1d4c0edd09d0285dab36cb93cd61ff  clip-groups.exp
bc230c6c5597e9dc614be6af160e9385a124956e44e5f00d9fa22cb509a353ab  clip-groups.pes
93ce79ad1478008a7d54be9ab68f3b8819c6a0e3de8e6c3f96e45cfd379e7b1c  clip-groups.pec
525c0f64f3660c7af17acbc3001e67f547a0a452e67a43a1f6e0893af8f7e3cc  clip-groups.jef
08eed05c4028d43e1c5a33561d928ab0d6821b79b2fe894fa90febd3e7772149  clip-groups.vp3
9fdd3058740eb6bcb6c45528bac270676c1e85905a783576a4143b0a9f13877b  symbols.dst
3da3ab3ef70ce5ecd82f0c97cd3098666d5bfe998d1b718a47edf57243c8422a  symbols.tap
3da3ab3ef70ce5ecd82f0c97cd3098666d5bfe998d1b718a47edf57243c8422a  symbols.t01
a3fb3391a3dc35af8ed87c19f7a3f77ab9c184e963a0e9b3f14bcecddc989ff2  symbols.exp
282fcae2d1178e51b8035d500ea66ad871d206d6691bb332529dd3373da2f7ac  symbols.pes
7922859faadee1586f78a0c934b25dd20cee4fdfea14fa73bc24f9389b6e6854  symbols.pec
b9afafb1061a64649cb1cb57f4eb491dfff770cd29ba1991099397d8b0ccfb57  symbols.jef
4b9a49c0f0b30ccb377a5c900de35be5c0986abcc420cc24fee00cc62febe539  symbols.vp3
32e80670356e037517f7c55e08ad603e8f60699f142a24a2e72a6496949afcab  trim-after.dst
810ad7358d5a6b4f9bacd108e013029bfb3a31814fed33c75a4baddab07fe5fa  trim-after.tap
810ad7358d5a6b4f9bacd108e013029bfb3a31814fed33c75a4baddab07fe5fa  trim-after.t01
db3dd37acd9bdb7a233814f096ba5b1ba6a66b2cc7b44ce5b95f8444d669eb50  trim-after.exp
267bf3c85147e8a25e9072d8f83a201ee9eb6bbadd5b241c77c4c49fc228318c  trim-after.pes
ce662c172e4552ce72166348e50e153b764c870ba244144f2fd8cb614d2fceaf  trim-after.pec
3fef6c163b2cb9a68e60a8c992ed412926618235bb4951526fc82e210b4aa060  trim-after.jef
18a9b31e09496349f130efbecd392189bfde4654c92a48d4dcc6d3fff435918d  trim-after.vp3
a6db3639f6297e6adc85e58870d4d902b13c7a777f5c0b617e73ec247535192b  opened-design.dst
ed3bcb8827c430234947f94f100ceef0e640b5354f74da2b4b1ddb1add1e8a04  opened-design.tap
ed3bcb8827c430234947f94f100ceef0e640b5354f74da2b4b1ddb1add1e8a04  opened-design.t01
411ef67683c67f89b56c4f7517dc7824050e421a0fff3a3741e8c24abfdb8f2f  opened-design.exp
0973b7f9a3de6d6e3b42165272a59498c2696e6e7addaaecbcd324e784fcdfb5  opened-design.pes
c63b130fae365738573be1e52d3dba82a6a024a816ec064bc8270dd87cf7f052  opened-design.pec
f5f2844f1c6a4161f60fd41957b5892ea45d8032c14a713e1200525c8ddddf67  opened-design.jef
95ed6272d246443e7a59cd58f9a3f81e708b8b0bccc42fb23df7e37a417052bc  opened-design.vp3
d19e0bad2a64785f7caf915a8ecccf519a2b93b7c392432edb9d2dfdf32af8da  opened-design-clipped.dst
8c862cb59aec17c1f4d945c22f155a8eb6298d675f6e573d7ac21d3ba810fe11  opened-design-clipped.tap
8c862cb59aec17c1f4d945c22f155a8eb6298d675f6e573d7ac21d3ba810fe11  opened-design-clipped.t01
0bc5398662184fed8442f0aa225ddd91a9d432e1f3fec4f2eaa672ed56fc4ec9  opened-design-clipped.exp
79d21868bdddc06b14ad80b7bd924839342cc563fd5f7ecefe716cf6239ba7a8  opened-design-clipped.pes
a8946d08d7834fdf8e40f7d5185bb887ffe8a27ba8d48edc58b24fee906c2b32  opened-design-clipped.pec
c0ed172b8a9eaf762ef246168e5266365b8757dfd87f660149e9f160b7360c45  opened-design-clipped.jef
a4a74b42e4b38a670eedf06bb20d6d790376ff13e3507ca0f3c884bff8125635  opened-design-clipped.vp3
0ac667d8c405ccf71442f9d2ef8fe450d60cba77786ab14feaff61f8737653b3  opened-design-cut-at-block-ends.dst
fce192774f923868bdd5f800fd2afb9e03ed6f0d62c4cd2577b3029f1174830c  opened-design-cut-at-block-ends.tap
fce192774f923868bdd5f800fd2afb9e03ed6f0d62c4cd2577b3029f1174830c  opened-design-cut-at-block-ends.t01
0d21bdeaf446854ad17e517fd98c3fb1cde79dcf16e1182b8438c63c80673bb6  opened-design-cut-at-block-ends.exp
326f4d74c9c76f053fdc11c17ad0a4d0d3617a8e72f0ab6be3fb6d634b63ed98  opened-design-cut-at-block-ends.pes
3c069140aa070e4e345ebf6b8ccd9d976807b81683ed7c19d95f270c2eeb0a68  opened-design-cut-at-block-ends.pec
d1f49d8273152b6064340076ddab9b45d4b3a2077864814a477ef799534ec262  opened-design-cut-at-block-ends.jef
d0c5af020602f9e85c6b002d4d122c14fd4e4e84a18897d2f3e620ea08f9af4b  opened-design-cut-at-block-ends.vp3
8ce0a0cf7ff2192f4b45d247ce71e45fc14767246d6187152002f4cb52b4ebd7  mixed-stitch-style-fill.dst
8d9c7e47052614a11f4fc514cdb868b1fabb7ecf3dce3277a430abc75ea93374  mixed-stitch-style-fill.tap
8d9c7e47052614a11f4fc514cdb868b1fabb7ecf3dce3277a430abc75ea93374  mixed-stitch-style-fill.t01
63c94dcf51ff773159a34f3ffa270da693018f78febd954c22afd0554514e4e3  mixed-stitch-style-fill.exp
4bb40aa05560299c0e75620b8831609c719c4c016f1d00b962a447f25c49a53e  mixed-stitch-style-fill.pes
defac7efbb3cb5a2bfd9ee26f35b557629259de48feac96a306febea76718870  mixed-stitch-style-fill.pec
7a7518dd5948604c300bf1532d236d627a01e38e44e3772b7ce4c14658b9fbf2  mixed-stitch-style-fill.jef
d46468a9da2fe49ef8d177140eab1e5418eba9797af520bf3836eff86f215297  mixed-stitch-style-fill.vp3
94c67ca4565acd37d9be943b2fb32961671756f3b692d1b1e4352ba56fd8b5a9  mixed-stitch-style-satin.dst
5f3a65f9bf9a5f34d183845874eca10ecc09b1e0de6f654c394e42ed8658ef89  mixed-stitch-style-satin.tap
5f3a65f9bf9a5f34d183845874eca10ecc09b1e0de6f654c394e42ed8658ef89  mixed-stitch-style-satin.t01
c25a33e508b130173a56bf8a14ccb59c6fd8ef648bf69327c83588b85abcaad5  mixed-stitch-style-satin.exp
51e72bca78719c9ceb3bd9e6289acff30920ba7679982eb28c7f1fbd1244868f  mixed-stitch-style-satin.pes
a796274bfbd3b493454626a6b18c2baa7c4238dec5bcb2764375e2a950a3bb48  mixed-stitch-style-satin.pec
50ba5e920f6a6ca3f240013005c306d80ab5caa160749977e093273a2391e8f5  mixed-stitch-style-satin.jef
053446f9082e16c893259670bd2bf3b614518eb6ccfb3c427eb1928b327bb342  mixed-stitch-style-satin.vp3
1cca1eb83b7fe0c7bf27c150c7864902199ad04d53b6f9453f874d6f5b3e7da8  mixed-stitch-style-running.dst
3090211233875177c2cd028d6f0f725937a653ac443e2dc06fbb6584f8eb3cf7  mixed-stitch-style-running.tap
3090211233875177c2cd028d6f0f725937a653ac443e2dc06fbb6584f8eb3cf7  mixed-stitch-style-running.t01
73e51eb6717a71e52022825238269f9b8f8dcde58cb59b0bb0b7c6df42558193  mixed-stitch-style-running.exp
f3a4e11146401c2965915cefb3667370c99d912a2f62e64d972769970dd3f940  mixed-stitch-style-running.pes
2e2dc1bbf5415fce56acb14deb6a627d57eb8b50c584adf2c754cef53f638051  mixed-stitch-style-running.pec
7c614001107a9398f52245e67bf497597178eabb562d3cc9d240b294d79a748a  mixed-stitch-style-running.jef
9e675c606bcef2c6b421dfcc2b2046697f55a1ebebb9f26e52dd73df0e1bc62f  mixed-stitch-style-running.vp3
bad1f8ce7c0d0d7e95511e61a60f1cfebb15c74eb472b6a9f558eda8d5253c6a  mixed-every-option.dst
2479751ee8f42cb659ce52a6c6037e43e19f8ac041314162676911535e5cbfe6  mixed-every-option.tap
2479751ee8f42cb659ce52a6c6037e43e19f8ac041314162676911535e5cbfe6  mixed-every-option.t01
b825e470d14bce6a48b8222a5184f5a25671c30438629c84cf8273e773d44c9f  mixed-every-option.exp
3f4b716a467d1cef42a18733e8734ad530330caca65df4ad0890cd77796ccb8f  mixed-every-option.pes
a4603c6a1ca1c6023a9b474cf012cf7fad486bbe99eee812bbf2b84932a1de43  mixed-every-option.pec
dccdb00e425381135d60c3ce3f80ae78917d74fa4fc9964b01e646c203bee6d3  mixed-every-option.jef
6b1a869fce7d5973e16abcb6c951ef73631f23c139809fedc008611eb4a21e8f  mixed-every-option.vp3
21c157da29eb3d6a8a6396ddce9e7adf2bb410b10dedd367d953292b24765488  mixed-shrunk.dst
a1351002fae5ad8dc949c236175aacfde66dfc9a8470dd7739f83f2f4e18f0ce  mixed-shrunk.tap
a1351002fae5ad8dc949c236175aacfde66dfc9a8470dd7739f83f2f4e18f0ce  mixed-shrunk.t01
0a104f6e7e7ba412e57d73129f3e033c912ac4f3d6a5afaa646328274703030f  mixed-shrunk.exp
358cfc25812f60c786edf68cf33741aba68f27161aac4ba935a6605b23270f29  mixed-shrunk.pes
7825bca668c22aa167c6c12078d6b39f1f495ed984d63d35f1c4ae865d269146  mixed-shrunk.pec
70f1819ad4d9de4c11576771831e2ddc3dbd30183177e697ce5225cdef891988  mixed-shrunk.jef
b8fdec1f486885631f34ba8a8f92f4c2717f55c1a7473d0ca8e2b8e6fedf0cb3  mixed-shrunk.vp3
";

/// The real designs' artwork at the person's size ([`DESIGNS`]), listed as [`SYNTHETIC`] is.
const ARTWORK: &str = "
e97a98f13925a9de2fc040715cb5da240aa73654cab82666d9fa4a883f6bf4ae  bullseye-twemoji.dst
32fbdbb01ba615c9d1fa57f0cd2a6ead3a9e3dec58ded8ea8171acfd72075c70  bullseye-twemoji.tap
32fbdbb01ba615c9d1fa57f0cd2a6ead3a9e3dec58ded8ea8171acfd72075c70  bullseye-twemoji.t01
c51c9749e9f32c67ae6de91afde092a99c374e558426c1db23d1479bfa385f58  bullseye-twemoji.exp
b8bab1efdfaadbb3f2321e91da9ef644b1ebbf6d02e88ea8de952f8e640324a0  bullseye-twemoji.pes
04643068c9ab7408f8c042fc78369a67487dbc439fc77b058510ee359e9a9ff0  bullseye-twemoji.pec
e776314f29f77149815e58b5b26589d17cdf728870c0aa8ffc2dc193ba7c44c9  bullseye-twemoji.jef
5d0839e6f2e54ec096b9233530a7384b871d557ae6d7dfa464b2e85ba89e6e35  bullseye-twemoji.vp3
021d45f4f267cc95d15a81983c83560a397a415bb3675587b0c85af2deeee4ae  fox-twemoji.clipart.dst
f99e87a8f437117a4c0e0c934bc95e01536dca9517cc70dec5b794eca807192d  fox-twemoji.clipart.tap
f99e87a8f437117a4c0e0c934bc95e01536dca9517cc70dec5b794eca807192d  fox-twemoji.clipart.t01
4e01f04347e77071127a8ae5f7ca0f734695a61953f15de7d1ac714b9f9fcc53  fox-twemoji.clipart.exp
95fab010000534c9209d2b5b6f2d61cb644a557c6ddf35519ffca88298d62dd8  fox-twemoji.clipart.pes
0ae97a2c5cbadbacfa51abceb367b6e8a81e219d124b5abd4bbf7b57d6f91f83  fox-twemoji.clipart.pec
9fb0ce4f2e46f60659af8427f62ebc872e1b26198c1bc4fdb9400f9cada70db3  fox-twemoji.clipart.jef
a5a09136d1fa3a236cbc5cebae2a0209c2b3709c3bef444a254a7552d647e03e  fox-twemoji.clipart.vp3
fa3bf827927ae2fa97044d2bc5378f829ecc3e3d0764533c3e012b654e75d518  heart-suit-twemoji.clipart.dst
0e99687778100518425f645cb6f9b91b83c2a288683e1b5bce30bcf36a983d87  heart-suit-twemoji.clipart.tap
0e99687778100518425f645cb6f9b91b83c2a288683e1b5bce30bcf36a983d87  heart-suit-twemoji.clipart.t01
7fab28677ab9a641c7cb93396566a566303b863ab682bb2c4e4fa429503ca105  heart-suit-twemoji.clipart.exp
afe03455c2e3e482193d8caddc0dbe90eac05b05be12e52b3160c0338eea0d06  heart-suit-twemoji.clipart.pes
13c8216e75fd1379d83abbe88e4caf6e1399b7b2a7e4ebd9b470042e9bb246ac  heart-suit-twemoji.clipart.pec
310ffd4d8a70640d968103483f0ae4a8bd70a8226c1c23038cdb9b3da82ccd8e  heart-suit-twemoji.clipart.jef
cc8cb978330061699b0b3d233db73348e3cfa69d276d1392e350a06b39dd975b  heart-suit-twemoji.clipart.vp3
1d9c645de0172dc96cb04f543c4ce385eb7f70db86abebfd141768bf5e54f39b  joystick-twemoji.dst
dcf444a9bbc2e292732293acdda63f8bd0b4cce0ea5dcaf9b22d81b3ad69bc93  joystick-twemoji.tap
dcf444a9bbc2e292732293acdda63f8bd0b4cce0ea5dcaf9b22d81b3ad69bc93  joystick-twemoji.t01
f05c7e48e12b5df6ec1e675923de3cd2a9e654503ce2f412336e30301750b0ca  joystick-twemoji.exp
e1dc552343726e57ca3df02f1621c8ebf742c4b8f6eb5c816ce0d8f03ce4f1e8  joystick-twemoji.pes
859017018b5ea29a7dc6acb54f1dbdf898f79bec62651210a42aba615141ecaa  joystick-twemoji.pec
35e189d08a7d3a484cc2a5ec7b9a1ad359e42fdcdd05b1cca06e8117ecf593c3  joystick-twemoji.jef
13af70df0cde065fa8ab286fc3e85c2c36e0c2d9e4dc6afcc91f0b9fb4e6a9d0  joystick-twemoji.vp3
5df98ea8be6d4a5d9e0407a496cab7ef8a50f29a7b3a61a0a3bed2d3304f64e3  pool-8-ball-twemoji.dst
22b12806cb0695f41efa9a908c0f3d9639e6d67a19436b29d43295646e058ad0  pool-8-ball-twemoji.tap
22b12806cb0695f41efa9a908c0f3d9639e6d67a19436b29d43295646e058ad0  pool-8-ball-twemoji.t01
6d1d2bd5860233af384e2b6436384247ee23a5aceddcef3a69c848e236c55103  pool-8-ball-twemoji.exp
fa2afa94c78b8e7efb9024eb4cdbd055781fbaf8f53b6e6dad73694d5df83748  pool-8-ball-twemoji.pes
dcd103f044729f7f4df98ce4507a49d788ce14cd22f11ce3015a8fcfcccc8f9c  pool-8-ball-twemoji.pec
1b39eeb94e5a0396986e8f187c620591c0adb5188a7ecfd462eeaddf73c4590d  pool-8-ball-twemoji.jef
ec5e3e9c23f221306dfbe980bf4b8c0b770d799bad837e789547e53cb0761d2d  pool-8-ball-twemoji.vp3
49472f3cb3298b0bb17a38715b3aaf2fac3e8ffb36ee7d7286179db44a692426  puzzle-piece-twemoji.dst
72a4e98783cab67742646e38e253ae49e989d72a14fb2e6d9e5e8c06ccf34674  puzzle-piece-twemoji.tap
72a4e98783cab67742646e38e253ae49e989d72a14fb2e6d9e5e8c06ccf34674  puzzle-piece-twemoji.t01
f4c71b20c87bc7163cae775dcfe8b7dcf628695a77ed130c145728af63d00d44  puzzle-piece-twemoji.exp
e28747c4bfd440a1df7f0b1261cabc700aa1d937956b7d918e35ced67f2669c1  puzzle-piece-twemoji.pes
cf224ba2bb3c44c30b23a7dfa718e5d98dd05fab00eb3cf52f7889e45da96352  puzzle-piece-twemoji.pec
3face538a9f778e687d8166f866e9f8290d22c43fd1bb350b43ebdb7ab4bac6e  puzzle-piece-twemoji.jef
ad47af0bec122a76c13decc362904693fe57e108e100728bfc2672f911bd1dd4  puzzle-piece-twemoji.vp3
4204b887555cf61f17667d6f1c410eb0d175e0548b3ddf44a6a87a659a2c96ac  smiling-face-with-smiling-eyes-twemoji.clipart.dst
06815399d13362d66c7b53c1c5a1e88c8670eec79a62e450d1a8d93111ed6114  smiling-face-with-smiling-eyes-twemoji.clipart.tap
06815399d13362d66c7b53c1c5a1e88c8670eec79a62e450d1a8d93111ed6114  smiling-face-with-smiling-eyes-twemoji.clipart.t01
a26d89a969bffa9fd2bf9a3ff03530e9a363400c8f9d161701cf144de6c144ea  smiling-face-with-smiling-eyes-twemoji.clipart.exp
942e45937ffbcd1ebfcd1ad6d95e43bf950becd14b4f02244836532298ba7efb  smiling-face-with-smiling-eyes-twemoji.clipart.pes
79305ce83565a57b687e4589ac23c565439ca27dd1ac07904724881b8d611d05  smiling-face-with-smiling-eyes-twemoji.clipart.pec
a55b93114edf7da567d513030ac2b1643c64c52389ead9dc1b0cd56298a25b4f  smiling-face-with-smiling-eyes-twemoji.clipart.jef
2d846f12185eda7962aa47efe67b17be3bd15992dca767057807e34c78c37abd  smiling-face-with-smiling-eyes-twemoji.clipart.vp3
89283bf75b3b96349e343ad25aea930f5c20f5f52e70c8118e398f321d88c381  sos-button-fluent.clipart.dst
10d14c02cb385ba2d0fc1caabed556eee3662cc0208d92e0c85928f6380b89b3  sos-button-fluent.clipart.tap
10d14c02cb385ba2d0fc1caabed556eee3662cc0208d92e0c85928f6380b89b3  sos-button-fluent.clipart.t01
1c879a598110d63b922c87c28b3c52ff574e3286651ebea6272758f07ece25b2  sos-button-fluent.clipart.exp
2b22825575b483099c820dbb7c68e9fa78e37c2117280d8095274fd3f6e3342a  sos-button-fluent.clipart.pes
9ae6da12d4b46c767af4be00f4d7f14d9344ce5cc8bbbe591a8c679b653e86ab  sos-button-fluent.clipart.pec
4f0e2967092dcfb1e783fde848bc1055922afffeded4c6c06f63a12d3140bf27  sos-button-fluent.clipart.jef
9d3dd97b38762ffe6753319cef567570a6287c86feef63d662eea43c0a341242  sos-button-fluent.clipart.vp3
2f5ae24501d1965c158011924ce1c37ef164f23998fb0b1763de0825ecde9286  video-game-twemoji.dst
7441b8ace43c001a830b7b284b4275dd2a2bb1329464e4a1e41c9855301c9ae7  video-game-twemoji.tap
7441b8ace43c001a830b7b284b4275dd2a2bb1329464e4a1e41c9855301c9ae7  video-game-twemoji.t01
5f9fb13e084f981aca0d91444e4298073ae78f0517d39fcc37b768c04c084060  video-game-twemoji.exp
b5b516642769f7572de5694dab0f1fd7ecafc2ec9654c2a8c85aade1ee4b0444  video-game-twemoji.pes
b235e19fe27380b1b1fd7c00275775c7e593d93ab275c4c138afe0782b890363  video-game-twemoji.pec
5f6ad65082e12c1ff9108a4a5508888d648ae3bdc4454b0518fc589e52df837f  video-game-twemoji.jef
acbae62cfe07e250c8e1407545370f293ce37fd65ceef87bfc3dd5d38f103e73  video-game-twemoji.vp3
51c9cf7b4d96f9cad0bfaa3bb55af897129c0c988f0b84975fe105979f5e3490  watermelon-twemoji.clipart.dst
42961d6bf8c204968813fa3df09952f3d095ba69dfa63079e9cfeb90502ddad3  watermelon-twemoji.clipart.tap
42961d6bf8c204968813fa3df09952f3d095ba69dfa63079e9cfeb90502ddad3  watermelon-twemoji.clipart.t01
ab9339cfc9b18d2a43fb8878ae864a9daa62181a578056f105a436d9f0bc804c  watermelon-twemoji.clipart.exp
d63ba8b5cc794c88220dd821c9cfd5cb7812699db0384a42917f76acc468455c  watermelon-twemoji.clipart.pes
c592eb56dcc61943d54fe423a3bce71ee018e93180c7ef82d0ecfa26666b9866  watermelon-twemoji.clipart.pec
a6301c7f6d802607b38a7684a2a1c88388611cf6e79656e0c58c7daed57d7b6b  watermelon-twemoji.clipart.jef
255570359078c9f9e4b535cfd00096da68e15544563f91776a4c9e3737be72ae  watermelon-twemoji.clipart.vp3
a3543a02538c4c3c4b50650815e80df34247c220d4f7446edf4dee6b25063ff0  zebra-twemoji.clipart.dst
983a79b278d5a9805942a2ebb297fc255b162f9115986874a300b1dcd16ed1ad  zebra-twemoji.clipart.tap
983a79b278d5a9805942a2ebb297fc255b162f9115986874a300b1dcd16ed1ad  zebra-twemoji.clipart.t01
4ab758b75174c4bb8ca119dc0089ad4598ffcc2b2174741db1056df4b8fe5662  zebra-twemoji.clipart.exp
7bce0a5aceb22fdf46ee7a7a9f7249a2db126e31b75d6a6c4dfb748b53657939  zebra-twemoji.clipart.pes
dfdd602bb0878f85e0f3ad9cb68302914b957a67d94aa02ae5911ff0065e66dd  zebra-twemoji.clipart.pec
0f507c118cb5203117e3e6fc0b9374e9520e7012d01be16c3e900cb0f16f59ef  zebra-twemoji.clipart.jef
d718dc1f5a02a24e2869aae424446b6331acc0064044397383d2390438a1e719  zebra-twemoji.clipart.vp3
";

/// The PES, PEC and JEF exports of [`SYNTHETIC`] apart from their thread numbers ([`threads_apart`]):
/// each one's hash with its thread numbers blanked, its name, and the numbers, in sewing order.
const SYNTHETIC_THREADS: &str = "
4458a9d441e3618a0b8c4c7f8a365c0b23e27b5f60d8f3a1b4d15a26c8c93beb  fills.pes  5 21 37 5
56fefb39d26cd37eb935dc16a197ee379f3365ae2d9fa95277ccb4b4d13fbbc4  fills.pec  5 21 37 5
3bba2e628dee41e9333883647b51909e577534748fd3bb4cccf5ad49533e7bbe  fills.jef  33 59 45 33
303bcde2063222873243edc349ada2646c3601227820a23eab9137d1e99611d0  strokes.pes  5 21 7 37 52 24
86449092eea76037da4fa97de477f8f3e647cb4bde009252754da6a31844c9f9  strokes.pec  5 21 7 37 52 24
0f23f8f6c391096553cca21a3f6c0fd6872c0d51b55dbf3d69cddbd5c59b03d7  strokes.jef  33 59 59 45 71 14
6193fe62222fc954cd78e5adecb733074957319da98eda1081563319ed24b8d8  compound-path-with-a-hole.pes  5 21
620b104cbed55a3d20c345add63b03719085f1d26b883cf9c01df6f021b56a14  compound-path-with-a-hole.pec  5 21
4dc237a56c66b14d036cc0f8e718f531b36b3b477c01e01331ef3a8cbc32ee8d  compound-path-with-a-hole.jef  33 59
4eeb291731464a4c2b21fb1bbb0e2562b48a849d2c7e17f2588fc31428a4dafb  clip-groups.pes  58 5 21 37 23
1bbaaaf3a12a53827294bf98da42bf1b53262441cb772db5b9ece486bb5546d4  clip-groups.pec  58 5 21 37 23
2253ed4207281892bd546b0eb0417aa09bf6c19616f8136ccf52d9a3d24e2774  clip-groups.jef  37 33 59 45 1
0c27ea2ba8962aed541400e951ba02263ad0cb9077e4edecb0e178f252c18697  symbols.pes  5 21
b8f3944987f0cd171e9118c6784e04261be191c52900a25f81fffb8db646b0c7  symbols.pec  5 21
a8193f55ac29094f84cbc67af92d02a9ab204db6c9b2d6c3a7d072d475d0ee99  symbols.jef  33 59
f0262b8f45c3c2bfb744f3b4bbde7fb7b913d9d50b57ae83a55c0e2e3810be4c  trim-after.pes  5
613034860c17cb4768b482531beab3aa732e98e940aad13fd51a16f2ea14039b  trim-after.pec  5
192823d8623f7f3fbce9089fb9318d004feeeb8ac79ec4cd5cf4b3c192d9f9c7  trim-after.jef  33
489b690d9ca2dee33b1bb560329dd765ab5aaa43cf6f9cdb653a992c7837ef09  opened-design.pes  5 21
9549f7ef5043323a7879317ec1b50003b74eb0de6eb09d3fc4ae1043b72378a0  opened-design.pec  5 21
f8d65a62ebb85a12f403cd2d222e44896e2e196929108e3138203e854d98d6f1  opened-design.jef  33 12
466cd315008511fa7d3c1ea962c5845b68e8525abf31392000ca72656f17eeb0  opened-design-clipped.pes  5
4a5ccc931b9a1f17d293f49560cf8c64a8debb3d076deab88002507628f367f2  opened-design-clipped.pec  5
be992ebec030b544d10545200539fafa55a4015393c5a38553cedf7238ca503d  opened-design-clipped.jef  33
d0a2834517c3873230966c078a804f4c22cc1c56dd2b599c126f005c523fcf0d  opened-design-cut-at-block-ends.pes  5 55
25bb97db737b1e2367a42ac7dbde6958d2b9112f04485d3da76b4844c1acdd63  opened-design-cut-at-block-ends.pec  5 55
e6effa654bfa5ad9d95db6df0c25e8de2686eed3f5ce3e9d6fa10dc8c1715f10  opened-design-cut-at-block-ends.jef  33 59
7126a4751a5b3d7ff87a04f6a944ea87dbf326697190857633d209e16a5a1768  mixed-stitch-style-fill.pes  5 37 21 7 52 24
ba03bc451b059e597e93b173ecd95514ef99efcce998c660b9e60c57282b658e  mixed-stitch-style-fill.pec  5 37 21 7 52 24
4fb4120120ccfe3ca857008d9539379d15eee3ff589b3998397317f201e9a374  mixed-stitch-style-fill.jef  33 45 59 59 71 14
1df1c81aab43fbfc3f3bc18c8a9349ef7a8f7737f32ed07a31e4c26279018d6d  mixed-stitch-style-satin.pes  5 37 21 7 52 24
8df7d9ae8cf61682790456fbdd7bade62adff2b2750294bfd736d777d83f900d  mixed-stitch-style-satin.pec  5 37 21 7 52 24
9701ed54fa71c7abfa23fca5e9b6531ffce7afaeb9c5a935449aad64428f74bd  mixed-stitch-style-satin.jef  33 45 59 59 71 14
5ca89e30e3b72852ab493fb3d6fa552a2fbd93b5ce7f0b8083547eec0342859a  mixed-stitch-style-running.pes  5 37 21 7 52 24
2422c9c489751441c6ca3b2f758c15897b822e47b8634b6276e159f0a936885d  mixed-stitch-style-running.pec  5 37 21 7 52 24
022898c2d8df04d76dcc1a81b2fb306e030cb6a14dfdb9b228f2ffe4d69013ae  mixed-stitch-style-running.jef  33 45 59 59 71 14
ae4f42cd67797dfa51d5b550718cd46bdcb5e1ebe4972ed315d01111ce3e6ae1  mixed-every-option.pes  5 37 21 7 52 24
dc296ca72d7d22c1a68b59857968dc7cb0ef0c5c3d9246f6cdd25b55976ca0d3  mixed-every-option.pec  5 37 21 7 52 24
77e8b0cd5af5df68c0e2c934d2f1ca1553572c4fd2ab2fb602dd6cbfb1f47810  mixed-every-option.jef  33 45 59 59 71 14
797769ac592127fbb00a5cb1b0b54345e8cadbe858b5dfb8314340833b0addb5  mixed-shrunk.pes  5 37 21 7 52 24
ee2da31d166fd934405e7a7cb66dffaa0982bea47b2f2a04664a0a30c4f89020  mixed-shrunk.pec  5 37 21 7 52 24
68bb5e902b8fe6968a38da6b1ebc3d132c83d438bd234b0b5c7a2763da6752f7  mixed-shrunk.jef  33 45 59 59 71 14
";

/// The PES, PEC and JEF exports of [`ARTWORK`] apart from their thread numbers, listed as
/// [`SYNTHETIC_THREADS`] is.
const ARTWORK_THREADS: &str = "
2cedd64012748a2aa4e5a6ed2de4f7c722d8c671b44bc0efcdfbcf6a9af1da1a  bullseye-twemoji.pes  29 32 5 33 20 36
eacbaffeefd90bf24761c4cd1f5a86fa6a3a33dfb650415e77da002a97552730  bullseye-twemoji.pec  29 32 5 33 20 36
34ff98eac986f04b10eb5a81573a8a26dc17c47a86ebe14fc2da95b96eef2ba0  bullseye-twemoji.jef  23 28 33 72 1 11
4b33b6987378c453910066f7ec819559afa4fc6007fa5af442bb04a4901e5b62  fox-twemoji.clipart.pes  52 12 20 51
12b20ec067c8ab15246c7d467252309828d90634dbd19fed5df70ecfb1fb1b5c  fox-twemoji.clipart.pec  52 12 20 51
8705abcbebe302ffa707a9ec9efc6656e53345b841dadab9224d65856237675d  fox-twemoji.clipart.jef  71 13 1 42
02a18a4dfd8cb40589072c456fa319238462f9ec75df699faa51ee5ba8589f49  heart-suit-twemoji.clipart.pes  51
43c10267fa46ad59eb7efc304bd7b08c89f7c7c90c72ebb57c72c400ae5996c2  heart-suit-twemoji.clipart.pec  51
e139c500cb4719cf04d91612846fa4ee96359294effad0d84c066de3796504df  heart-suit-twemoji.clipart.jef  42
5f2a7b0b3e450f04f632ae3bfa8d3590f4a615b6e4ca9850a555b217839e8a9f  joystick-twemoji.pes  16 48 23 39
942c85d3422498c489c9f68e205f17228fec11405d355273a47a1ecf2198ecda  joystick-twemoji.pec  16 48 23 39
f03b28dddb982054b2b5c8c8b5da588e127f3b88b18bbce2080ec66642247e70  joystick-twemoji.jef  21 33 50 50
9971a3dc29efbb48654c83929eb83a88766f86bf0d9d06e44d87311d2398fa25  pool-8-ball-twemoji.pes  23 29 23
7925844bb41408289b1c48b891f7f367ff9ca44ab51b8030a5edfd52944f1f00  pool-8-ball-twemoji.pec  23 29 23
ac4feb425fd92d452aedee81c84e0c8c32a795559cbbe4c85a70ce9317361af2  pool-8-ball-twemoji.jef  50 23 50
41fa172e8a5f8274963e0f6944c4a564f646e713daf028e5ed73dd3c0679d7ae  puzzle-piece-twemoji.pes  37
34691806bb0237cbb48118168d82224065d78115fc05f388ae78d21f355b2a13  puzzle-piece-twemoji.pec  37
14d96bd0804577559e06268b551726bff008f558781446565b1daf7fd0dc6774  puzzle-piece-twemoji.jef  22
eb2859289c1fce0d2cb5c2daa16b334cafddc472c71cc1beed42d886bdeaca60  smiling-face-with-smiling-eyes-twemoji.clipart.pes  58 43 23
9c8db5ef7a21ff76167c6098bd8c10e2916a41800cc032cf9ac08d7892e39adb  smiling-face-with-smiling-eyes-twemoji.clipart.pec  58 43 23
74a3b22123d9e1ee72e45e139a1472a5092bd4223475b674c05ca21e3240279a  smiling-face-with-smiling-eyes-twemoji.clipart.jef  73 9 1
4b9a4bbb3cf3a91644567fe93fe93edf8b0b334ba52b5accba33a340d39448b3  sos-button-fluent.clipart.pes  48 29
06e2b9c92e090edfe61360bbaebe71ac9a8094b8eb13066e84100d0830ff0570  sos-button-fluent.clipart.pec  48 29
c2203917e2267480a65dfb193b03fbf3858818f18a4029655ef098a034710568  sos-button-fluent.clipart.jef  32 2
b435ae4ad4b9368cb3a87dbc96d193013c6fb847105a3cca66dcda49d3aee97e  video-game-twemoji.pes  23 20 39 12 37 32 5 39
ea7afa5250a82a06ecffc66b0afd2a4c820fe9e1adf1156ee8ed624653a3ca4c  video-game-twemoji.pec  23 20 39 12 37 32 5 39
22ce48950c578490299dd7341363f833ca1b199e0ff60aeb370d845591d81929  video-game-twemoji.jef  50 1 50 37 22 28 33 24
f4d315e1f4a7890ff45bece230f3b433316388eb92918291f3551d58d8d8108a  watermelon-twemoji.clipart.pes  54 29 5 20
2e6cdb4f0abe7a92cbe58f4de98f6e0e7fa082ea1ab35096abf1c0f430f74253  watermelon-twemoji.clipart.pec  54 29 5 20
b70f59efda8d08291afed96c988fac716b28684a703504d6c44d43d1da3cd146  watermelon-twemoji.clipart.jef  6 51 42 1
47cc70e41a878113a4619942e7666030996349861ce8dc96c64aad1345f5359f  zebra-twemoji.clipart.pes  29 20 29 39 23 20 39 20
17d85a312c4d57f9c3aeb10e82fd8c717854e07b85c775cb1ed9c0d48b9214f7  zebra-twemoji.clipart.pec  29 20 29 39 23 20 39 20
1249db9b6ed90c43680ffdf40e1e7fc2363f9ed714d7249539dfe00d9f8a0067  zebra-twemoji.clipart.jef  51 1 51 50 1 1 50 1
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
bullseye-twemoji.dst  blocks 6  needle 1562/998/552/230/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1716
bullseye-twemoji.tap  blocks 6  needle 1562/998/552/230/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1716
bullseye-twemoji.t01  blocks 6  needle 1562/998/552/230/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1716
bullseye-twemoji.exp  blocks 6  needle 1562/998/552/230/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1716
bullseye-twemoji.pes  blocks 6  needle 1562/998/552/230/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.52  shortest 0.32  cells 1716
bullseye-twemoji.pec  blocks 6  needle 1562/998/552/230/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.52  shortest 0.32  cells 1716
bullseye-twemoji.jef  blocks 6  needle 1562/998/552/230/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.2x45.2  longest 8.50  shortest 0.32  cells 1716
bullseye-twemoji.vp3  blocks 6  needle 1562/998/552/230/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.2x45.2  longest 8.50  shortest 0.32  cells 1716
fox-twemoji.clipart.dst  blocks 4  needle 1418/462/144/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.36  cells 1331
fox-twemoji.clipart.tap  blocks 4  needle 1418/462/144/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.36  cells 1331
fox-twemoji.clipart.t01  blocks 4  needle 1418/462/144/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.36  cells 1331
fox-twemoji.clipart.exp  blocks 4  needle 1418/462/144/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.36  cells 1331
fox-twemoji.clipart.pes  blocks 4  needle 1418/462/144/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.36  cells 1331
fox-twemoji.clipart.pec  blocks 4  needle 1418/462/144/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.36  cells 1331
fox-twemoji.clipart.jef  blocks 4  needle 1418/462/144/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.32  cells 1334
fox-twemoji.clipart.vp3  blocks 4  needle 1418/462/144/248  jumps 1/2/3/2  trims 0/0/0/0  size 45.2x40.7  longest 10.04  shortest 0.32  cells 1334
heart-suit-twemoji.clipart.dst  blocks 1  needle 1356  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1369
heart-suit-twemoji.clipart.tap  blocks 1  needle 1356  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1369
heart-suit-twemoji.clipart.t01  blocks 1  needle 1356  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1369
heart-suit-twemoji.clipart.exp  blocks 1  needle 1356  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1369
heart-suit-twemoji.clipart.pes  blocks 1  needle 1356  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1369
heart-suit-twemoji.clipart.pec  blocks 1  needle 1356  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1369
heart-suit-twemoji.clipart.jef  blocks 1  needle 1356  jumps 1  trims 0  size 45.2x42.3  longest 4.61  shortest 0.36  cells 1370
heart-suit-twemoji.clipart.vp3  blocks 1  needle 1356  jumps 1  trims 0  size 45.2x42.3  longest 4.61  shortest 0.36  cells 1370
joystick-twemoji.dst  blocks 4  needle 155/247/1027/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.36  cells 1006
joystick-twemoji.tap  blocks 4  needle 155/247/1027/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.36  cells 1006
joystick-twemoji.t01  blocks 4  needle 155/247/1027/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.36  cells 1006
joystick-twemoji.exp  blocks 4  needle 155/247/1027/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.36  cells 1006
joystick-twemoji.pes  blocks 4  needle 155/247/1027/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.32  cells 1004
joystick-twemoji.pec  blocks 4  needle 155/247/1027/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.32  cells 1004
joystick-twemoji.jef  blocks 4  needle 155/247/1027/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.3  longest 5.70  shortest 0.32  cells 986
joystick-twemoji.vp3  blocks 4  needle 155/247/1027/229  jumps 1/2/4/1  trims 0/0/0/0  size 42.7x45.3  longest 5.70  shortest 0.32  cells 986
pool-8-ball-twemoji.dst  blocks 3  needle 1627/480/315  jumps 1/1/1  trims 0/0/0  size 46.3x46.2  longest 4.67  shortest 0.36  cells 1755
pool-8-ball-twemoji.tap  blocks 3  needle 1627/480/315  jumps 1/1/1  trims 0/0/0  size 46.3x46.2  longest 4.67  shortest 0.36  cells 1755
pool-8-ball-twemoji.t01  blocks 3  needle 1627/480/315  jumps 1/1/1  trims 0/0/0  size 46.3x46.2  longest 4.67  shortest 0.36  cells 1755
pool-8-ball-twemoji.exp  blocks 3  needle 1627/480/315  jumps 1/1/1  trims 0/0/0  size 46.3x46.2  longest 4.67  shortest 0.36  cells 1755
pool-8-ball-twemoji.pes  blocks 3  needle 1627/480/315  jumps 1/1/1  trims 0/0/0  size 46.3x46.2  longest 4.60  shortest 0.32  cells 1756
pool-8-ball-twemoji.pec  blocks 3  needle 1627/480/315  jumps 1/1/1  trims 0/0/0  size 46.3x46.2  longest 4.60  shortest 0.32  cells 1756
pool-8-ball-twemoji.jef  blocks 3  needle 1627/480/315  jumps 1/1/1  trims 0/0/0  size 46.2x46.2  longest 4.67  shortest 0.32  cells 1757
pool-8-ball-twemoji.vp3  blocks 3  needle 1627/480/315  jumps 1/1/1  trims 0/0/0  size 46.2x46.2  longest 4.67  shortest 0.32  cells 1757
puzzle-piece-twemoji.dst  blocks 1  needle 1340  jumps 1  trims 0  size 45.2x45.2  longest 4.67  shortest 0.32  cells 1207
puzzle-piece-twemoji.tap  blocks 1  needle 1340  jumps 1  trims 0  size 45.2x45.2  longest 4.67  shortest 0.32  cells 1207
puzzle-piece-twemoji.t01  blocks 1  needle 1340  jumps 1  trims 0  size 45.2x45.2  longest 4.67  shortest 0.32  cells 1207
puzzle-piece-twemoji.exp  blocks 1  needle 1340  jumps 1  trims 0  size 45.2x45.2  longest 4.67  shortest 0.32  cells 1207
puzzle-piece-twemoji.pes  blocks 1  needle 1340  jumps 1  trims 0  size 45.2x45.2  longest 4.67  shortest 0.36  cells 1205
puzzle-piece-twemoji.pec  blocks 1  needle 1340  jumps 1  trims 0  size 45.2x45.2  longest 4.67  shortest 0.36  cells 1205
puzzle-piece-twemoji.jef  blocks 1  needle 1340  jumps 1  trims 0  size 45.2x45.2  longest 4.67  shortest 0.36  cells 1204
puzzle-piece-twemoji.vp3  blocks 1  needle 1340  jumps 1  trims 0  size 45.2x45.2  longest 4.67  shortest 0.36  cells 1204
smiling-face-with-smiling-eyes-twemoji.clipart.dst  blocks 3  needle 1880/368/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.36  cells 1858
smiling-face-with-smiling-eyes-twemoji.clipart.tap  blocks 3  needle 1880/368/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.36  cells 1858
smiling-face-with-smiling-eyes-twemoji.clipart.t01  blocks 3  needle 1880/368/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.36  cells 1858
smiling-face-with-smiling-eyes-twemoji.clipart.exp  blocks 3  needle 1880/368/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.36  cells 1858
smiling-face-with-smiling-eyes-twemoji.clipart.pes  blocks 3  needle 1880/368/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.90  shortest 0.32  cells 1859
smiling-face-with-smiling-eyes-twemoji.clipart.pec  blocks 3  needle 1880/368/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.90  shortest 0.32  cells 1859
smiling-face-with-smiling-eyes-twemoji.clipart.jef  blocks 3  needle 1880/368/388  jumps 1/2/3  trims 0/1/0  size 47.7x47.7  longest 5.92  shortest 0.32  cells 1860
smiling-face-with-smiling-eyes-twemoji.clipart.vp3  blocks 3  needle 1880/368/388  jumps 1/2/3  trims 0/0/0  size 47.7x47.7  longest 5.92  shortest 0.32  cells 1860
sos-button-fluent.clipart.dst  blocks 2  needle 1989/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.41  cells 2079
sos-button-fluent.clipart.tap  blocks 2  needle 1989/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.41  cells 2079
sos-button-fluent.clipart.t01  blocks 2  needle 1989/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.41  cells 2079
sos-button-fluent.clipart.exp  blocks 2  needle 1989/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.41  cells 2079
sos-button-fluent.clipart.pes  blocks 2  needle 1989/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.40  cells 2079
sos-button-fluent.clipart.pec  blocks 2  needle 1989/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.40  cells 2079
sos-button-fluent.clipart.jef  blocks 2  needle 1989/770  jumps 1/3  trims 0/1  size 45.2x45.2  longest 4.60  shortest 0.36  cells 2078
sos-button-fluent.clipart.vp3  blocks 2  needle 1989/770  jumps 1/3  trims 0/0  size 45.2x45.2  longest 4.60  shortest 0.36  cells 2078
video-game-twemoji.dst  blocks 8  needle 582/1186/141/31/30/31/29/142  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.67  shortest 0.32  cells 1220
video-game-twemoji.tap  blocks 8  needle 582/1186/141/31/30/31/29/142  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.67  shortest 0.32  cells 1220
video-game-twemoji.t01  blocks 8  needle 582/1186/141/31/30/31/29/142  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.67  shortest 0.32  cells 1220
video-game-twemoji.exp  blocks 8  needle 582/1186/141/31/30/31/29/142  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.67  shortest 0.32  cells 1220
video-game-twemoji.pes  blocks 8  needle 582/1186/141/31/30/31/29/142  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.67  shortest 0.32  cells 1221
video-game-twemoji.pec  blocks 8  needle 582/1186/141/31/30/31/29/142  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.67  shortest 0.32  cells 1221
video-game-twemoji.jef  blocks 8  needle 582/1186/141/31/30/31/29/142  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.0  longest 4.67  shortest 0.32  cells 1220
video-game-twemoji.vp3  blocks 8  needle 582/1186/141/31/30/31/29/142  jumps 4/3/2/1/1/1/1/4  trims 0/0/0/0/0/0/0/0  size 45.2x32.0  longest 4.67  shortest 0.32  cells 1220
watermelon-twemoji.clipart.dst  blocks 4  needle 411/367/602/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.60  shortest 0.41  cells 966
watermelon-twemoji.clipart.tap  blocks 4  needle 411/367/602/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.60  shortest 0.41  cells 966
watermelon-twemoji.clipart.t01  blocks 4  needle 411/367/602/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.60  shortest 0.41  cells 966
watermelon-twemoji.clipart.exp  blocks 4  needle 411/367/602/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.60  shortest 0.41  cells 966
watermelon-twemoji.clipart.pes  blocks 4  needle 411/367/602/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.60  shortest 0.41  cells 966
watermelon-twemoji.clipart.pec  blocks 4  needle 411/367/602/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.60  shortest 0.41  cells 966
watermelon-twemoji.clipart.jef  blocks 4  needle 411/367/602/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.60  shortest 0.40  cells 968
watermelon-twemoji.clipart.vp3  blocks 4  needle 411/367/602/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.60  shortest 0.40  cells 968
zebra-twemoji.clipart.dst  blocks 8  needle 1565/322/270/42/24/567/46/769  jumps 3/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1572
zebra-twemoji.clipart.tap  blocks 8  needle 1565/322/270/42/24/567/46/769  jumps 3/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1572
zebra-twemoji.clipart.t01  blocks 8  needle 1565/322/270/42/24/567/46/769  jumps 3/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1572
zebra-twemoji.clipart.exp  blocks 8  needle 1565/322/270/42/24/567/46/769  jumps 3/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1572
zebra-twemoji.clipart.pes  blocks 8  needle 1565/322/270/42/24/567/46/769  jumps 3/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1571
zebra-twemoji.clipart.pec  blocks 8  needle 1565/322/270/42/24/567/46/769  jumps 3/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1571
zebra-twemoji.clipart.jef  blocks 8  needle 1565/322/270/42/24/567/46/769  jumps 3/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
zebra-twemoji.clipart.vp3  blocks 8  needle 1565/322/270/42/24/567/46/769  jumps 3/2/2/1/1/3/1/9  trims 0/0/0/0/0/0/0/0  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
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
        // Titled here, whatever title the document got: DST, PES and PEC carry the title as their
        // label, so the bytes must not depend on how untitled or opened documents are named.
        run(&mut s, "file.info", json!({"title": name}));
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
    let mut options = options.clone();
    // Every block listed.
    options["limit"] = json!(10_000);
    let stats = run(s, "embroidery.stats", options);
    assert_eq!(stats["truncated"], json!(false));
    let needle = stats["blocks"].as_array().unwrap().iter().map(|b| b["stitches"].as_u64().unwrap() as usize).collect();
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
    run(&mut s, "file.info", json!({"title": "opened"}));
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
