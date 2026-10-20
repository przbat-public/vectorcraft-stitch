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
use vectorcraft_stitch::{Block, Command, Pattern, Stitch, Thread};

#[path = "../../../xtask/src/sha256.rs"]
mod sha256;

/// The documents built here ([`documents`]), as `sha256sum` lists files: the hash, two spaces, the
/// document's name and the format's extension.
const SYNTHETIC: &str = "
71cd9afb0fca475aed5c4d241a59c4da0efa11a21e820024c03b024f24e59cf2  fills.dst
48e479fed9c97885efa0cb5a23cedd1562f21c8ea00ce3da234d9e218614ee5e  fills.exp
6f5ddffc298b3a071d733f9e09c95acdcd009b100c41a619c908f07a4840622e  fills.pes
1742c4e9c7b3af4caaf436b248a993e226ee61a8b6c935da16a3cc009e1b90fa  fills.jef
781dcd0206a94a7d8b8ef21868d9fe2f40b6be06d7accdf791b5657776125460  strokes.dst
4dc8987b8419f8a675631f659bef658a852429ab2608d4c0652a9ab3202dd3a7  strokes.exp
9e9fb5bd6d364aee34d8c198c4b3867f1c0bca4e022720d67eb5f300d4ee8a22  strokes.pes
3536c85add25a434ad992d9fcb0b86518c4bfae1083a3571fa650b3b13ef23e5  strokes.jef
c9e180dd033b677e9159c8409185d04906f59880c4de63b8edd2454af3f001cb  compound-path-with-a-hole.dst
6eeb8fc77450233e9b598274189424b8bcb6cd8565a6a1669ab50f90a87cf7d1  compound-path-with-a-hole.exp
8ec13cdc1219e082d171741083110a08e326c6eb43f36cec1363726ac5f8f4c8  compound-path-with-a-hole.pes
ad6e0df849186bb15c432a400bf2d7c51b09e432baad039a338949e8af14a1ff  compound-path-with-a-hole.jef
f7152fc66bf48744dce86f5556c2a1631a88a77dff1f1cc09857a2122fae2b0e  clip-groups.dst
88eeee00a5641ddaee652b3d1c3b06ca8f1d4c0edd09d0285dab36cb93cd61ff  clip-groups.exp
be0bd1a0d7b892dd6ed41cda887e1b09ad4ecfbd7c66a0f5f93e541f03caf244  clip-groups.pes
718bba9ddfac1bbe5e4a3bcd2b9ea92aa4c0977a129b74029bdefb2b4b5ef262  clip-groups.jef
2ef76f9a8469c9ca20fc8ef0dd864de4694ba50d0c62a992937ebb4c9b3c3ed0  symbols.dst
a3fb3391a3dc35af8ed87c19f7a3f77ab9c184e963a0e9b3f14bcecddc989ff2  symbols.exp
51904f7847a094758a80696f7f7622179a023ddafab6edad968cd0b72fb51c10  symbols.pes
c709927805dc9dfdec516a14a8e2c7be0d6b128a6d9cad8edd5d2fcc1d642564  symbols.jef
0aac0e1762f49b2e06a80d6798058acd642eae97ae676a6c5fc94a6939eba66c  trim-after.dst
db3dd37acd9bdb7a233814f096ba5b1ba6a66b2cc7b44ce5b95f8444d669eb50  trim-after.exp
8e02299d7c710a2c36c37731db2e768a18c50bb99606489ba5aaf40f02ba9936  trim-after.pes
3fef6c163b2cb9a68e60a8c992ed412926618235bb4951526fc82e210b4aa060  trim-after.jef
3770f862cd00ff1348368f702baca727ce4c7c3df61d0f6c9b7733694bfb7150  opened-design.dst
411ef67683c67f89b56c4f7517dc7824050e421a0fff3a3741e8c24abfdb8f2f  opened-design.exp
646826bb6371dfc4a49f2c32b4907f2052209aff6c414399b9cf3f9e0827af33  opened-design.pes
6259aa9deeb51f1daf428890257288ac8673194c8d98b60add8a6bffce3e2445  opened-design.jef
08ea1acb0ca703d79a5a6aff95df967dcb8a7e885d256c0848fad34288a6c192  opened-design-clipped.dst
0bc5398662184fed8442f0aa225ddd91a9d432e1f3fec4f2eaa672ed56fc4ec9  opened-design-clipped.exp
b648162355c0625a2b88c9b13ae75939c0dcaa4fa0548347fc3dddffed465abb  opened-design-clipped.pes
c0ed172b8a9eaf762ef246168e5266365b8757dfd87f660149e9f160b7360c45  opened-design-clipped.jef
60779f82a2627325eca8218f44549ba02b67a8384e13430efdd7e61f7d4c3f71  opened-design-cut-at-block-ends.dst
0d21bdeaf446854ad17e517fd98c3fb1cde79dcf16e1182b8438c63c80673bb6  opened-design-cut-at-block-ends.exp
57167efb9edb824a3184cc92a268cdea1c4b72b3b4d63ea581cd93c3497700d5  opened-design-cut-at-block-ends.pes
5c103a60e81962bb576a69ad5531cdc34e4c47c2992d33c2338024cd1d3d60af  opened-design-cut-at-block-ends.jef
f8554bb0c22deb3d77faa14b94c417d85dacb16530ad7f48d13040afeda920c1  mixed-stitch-style-fill.dst
63c94dcf51ff773159a34f3ffa270da693018f78febd954c22afd0554514e4e3  mixed-stitch-style-fill.exp
56e09b371804a23374d3e52f0609ceb79766e7b3d62ad797bc78cf6b52bf5ef3  mixed-stitch-style-fill.pes
77d5f468da308db17f3505e931f73f280cf2e82eb0e7f3647fedf9646daaa902  mixed-stitch-style-fill.jef
5994e1310478ba976f3db9a33cf68d390ff35c9e5b3e5cd26b131b023d59e62c  mixed-stitch-style-satin.dst
c25a33e508b130173a56bf8a14ccb59c6fd8ef648bf69327c83588b85abcaad5  mixed-stitch-style-satin.exp
7ec5834e463bf1fe63976a732276e0e31a59bf771e5e1d60a07e3bc0065d79d1  mixed-stitch-style-satin.pes
45681fe40c4e7e730743430533c59490bd6b06f74ee67a2224fe0f74a63bad41  mixed-stitch-style-satin.jef
ac3236ff8f71c0801c2bf5d22185633c4e453cea1b6383bae2e64eecb99933dd  mixed-stitch-style-running.dst
73e51eb6717a71e52022825238269f9b8f8dcde58cb59b0bb0b7c6df42558193  mixed-stitch-style-running.exp
a701108263babb2b61de80ced0476de090f89bd0e911e74c38a65fcdbcd21f0d  mixed-stitch-style-running.pes
18564c6afdd151b402bfcfd3cc8cc8b851dcd709a89310ec7c88a21ef0111458  mixed-stitch-style-running.jef
628a42577dcafcc93cd6b1e4f9a433beba292374ab4f1b2f13eb3596b81ec5c6  mixed-every-option.dst
b825e470d14bce6a48b8222a5184f5a25671c30438629c84cf8273e773d44c9f  mixed-every-option.exp
b96a59ebe42a128d1d60d6c192d894b5b807d3e1144df0be0cd9cbd9db8e4f89  mixed-every-option.pes
442b7c177aaadc4c2c2fb16a73c3db7c9c11712f5a02b2be10b79eab1424b828  mixed-every-option.jef
f1360e29ffdbffbf6c40d265ff6fee22a136b3ce8bbeb7a56fc3f01541d194a6  mixed-shrunk.dst
0a104f6e7e7ba412e57d73129f3e033c912ac4f3d6a5afaa646328274703030f  mixed-shrunk.exp
530e8ba614cc386888b993482f66e780268920184f6e40cc2d8f230629591588  mixed-shrunk.pes
3276032028b02fa1f7e6d16c4e1a31b6591bc2139a6ef16cf006e439d3e76732  mixed-shrunk.jef
";

/// The real designs' artwork at the person's size ([`DESIGNS`]), listed as [`SYNTHETIC`] is.
const ARTWORK: &str = "
66117e41cc9494c46174ca97acaa9d966d4ccd47b11299ccd0e9695927019989  bullseye-twemoji.dst
80f93b52105ac8b7a5202f8c7e5b97e016c9618a23955253cf0892a15e38d2e5  bullseye-twemoji.exp
f4ef97c35ded833ea4780cfcdfc318b0c3b27a49cbc525138e6bed23acdd6347  bullseye-twemoji.pes
2ed0c62e42334fb81f3c2974d76777a8b93b47c6f1acaf8b6821ec78bd9e55b8  bullseye-twemoji.jef
4cf9c0f8c8dd217d5d3882fc7b8faeb3764254c07de1c698d935e609d33214f0  fox-twemoji.clipart.dst
2511a55c908af2653bf0acf386f8ae68ea943e35aab0e8fbb033212b549e42d3  fox-twemoji.clipart.exp
232cb4a4c7ce72413ee996c4bb2f36c01c61d7c646060b9421e3434d305dd612  fox-twemoji.clipart.pes
099fa5da1cd4b46ce97552ba7779d3cf339657cd520036a12812de8e4100511f  fox-twemoji.clipart.jef
7cb8031943f4228344ed0415ca2d061269f171a8dff30eb2afc18a9979658971  heart-suit-twemoji.clipart.dst
d505ae5ca0e445823b4ef58a1af1e06dc59304ddccd826e99923e52ec4a9329c  heart-suit-twemoji.clipart.exp
366b8cecc0c38c4dc2ad1d2eb4533a7b285b68b4331cd68a51cdc5a8ba5eca2e  heart-suit-twemoji.clipart.pes
0521334653d7f2592c1444c07aa597466ad8efb62e6a50690c384654a8a624a7  heart-suit-twemoji.clipart.jef
592e56ebca2a3106271789cc641fc68f60cc3c45b3370d9df23c20d4d51831f6  joystick-twemoji.dst
6979bef6e710634887f206a95faa11ef781aced99aa6a086095e6fe978e3ca10  joystick-twemoji.exp
961fa811268df7e8d4a07c9d9ca84e18b2f8914bcc4796464956979da90e2fb9  joystick-twemoji.pes
23f55fa8b5a25eb4082d3903a9d50a0674fff661ed4868c89f686c8eb093029d  joystick-twemoji.jef
a341dabfb46c084fb6879c651c0d8a219aa24386734be319373a337f05e6f748  pool-8-ball-twemoji.dst
88d56f86fa2f6827353074884fa544b2171235d47e2710b7b2244b75b5c04f97  pool-8-ball-twemoji.exp
98faa78a8ed67e35244de7faa42072185571cdfdb54d44975adf6d7b87a17480  pool-8-ball-twemoji.pes
908526ea160505e675ffcebea13324b3e8af5497457f7ece972f187725ce0a30  pool-8-ball-twemoji.jef
62ee4d5cbb70ddfab2594642ccda8dece153a706b31019683276df974d060b4b  puzzle-piece-twemoji.dst
2e70a6ee136df1e68ebe388131e20d5ab68c95ba53d4c6407f91cd7a34dbb0b2  puzzle-piece-twemoji.exp
3dab591f415666fe4d94177dfb2e949a48c1ae8835c624998c0ed8b8488b49df  puzzle-piece-twemoji.pes
6b3f26c749b6a93b6cfbaf293c18cd4a6a9ca75df344b4453a3821dd36c27c39  puzzle-piece-twemoji.jef
438c173f980524b83dfc0c58ad29149b89d1684b1a9ca8fd1361f470046a181a  smiling-face-with-smiling-eyes-twemoji.clipart.dst
296e8a552f52dc95745205436fd9b413edd28f97e69be3fcab07f108d29515de  smiling-face-with-smiling-eyes-twemoji.clipart.exp
cd656a9c0e63a16ab806fa0bf44b83640a2eb2e72a3e838726d816b986791c7d  smiling-face-with-smiling-eyes-twemoji.clipart.pes
38196ade9a75bd1feb12c950dc75684c0dc716c0d708d3d0d27a557929c24f82  smiling-face-with-smiling-eyes-twemoji.clipart.jef
6fe71459fb99479ade7b676db843fe802dc419380e4740f5d3ce4adb89d3580d  sos-button-fluent.clipart.dst
4dd6f6f0d087a68adcb8945d5bbf8fc8f8e8e3d0e98f3d913321fa8bd5386ea3  sos-button-fluent.clipart.exp
4945ab0941c8f046f7d700f0a33a07926a07f17e03bdbc1dad394496369c760f  sos-button-fluent.clipart.pes
7127600a9d722218c0cfc0b01718e0049b643c46ec8d3e0d206951b64eb82fa4  sos-button-fluent.clipart.jef
654c5ecddf7ece28673f6769d131c73c88478998bff2b9d44aeb70fa367ac62e  video-game-twemoji.dst
b25db9a38d1d6dc3695f3891fc394223b11a7403097a67e8c14fe404b0542a28  video-game-twemoji.exp
04a8f02c8e34ee0cd3e00c3cfb57e0d23c77819e037d79fd240e16b662abc204  video-game-twemoji.pes
6d61bba7a2d868bd988582e5d0d1dbcf171be16e123a0993b6f8bc92bdaa7f00  video-game-twemoji.jef
2484cbafcf1719125dea0a74ea395775175a71c77d26622c98fe24c6fbd874f4  watermelon-twemoji.clipart.dst
ac706c4fb5dc6d97861e9db1446df00c442c951a8ecfbff77dfa5daba95206bb  watermelon-twemoji.clipart.exp
67540acf7cd77e7dce5e0827750fa66564457957860f6786ce104b273c1c8338  watermelon-twemoji.clipart.pes
51ed7fdedf0038169fa76b4cab7340cd7530f3b5671ce71c97299c5f6bc63c2b  watermelon-twemoji.clipart.jef
ff2cfff1e53754f2e8ddc895ba78eb82a29aa5508d89d721c6a95379748b36c4  zebra-twemoji.clipart.dst
e4521143ee2d35458c2854d910bf3f098763ad89b27fe7aba31c786f6360476c  zebra-twemoji.clipart.exp
add034d064379cbf73c7ff51969ffff890c888bcd07e7a61c98294ccf93e5a69  zebra-twemoji.clipart.pes
9079a079ef6c95db0f3ffa65c4f41026200ddcaeafbd7f261805dcab7b45ee99  zebra-twemoji.clipart.jef
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

/// The documents built here, exported in every format, write what they wrote when the listing was
/// recorded.
#[test]
fn the_documents_built_here_export_unchanged() {
    let mut now = Vec::new();
    for (name, mut s, options) in documents() {
        for format in Format::ALL {
            let bytes = export(&mut s, format, &options).unwrap_or_else(|e| panic!("{name} as {}: {e}", format.id()));
            now.push((format!("{name}.{}", format.extension()), sha256::hex(&bytes)));
        }
    }
    check(SYNTHETIC, &now);
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
            now.push((format!("{stem}.{}", format.extension()), sha256::hex(&bytes)));
        }
    }
    check(ARTWORK, &now);
}

/// Every export's hash is the one `listing` records, and the listing has no other: otherwise the
/// failure names each change and prints the listing as it now stands.
fn check(listing: &str, now: &[(String, String)]) {
    let recorded: Vec<(&str, &str)> = listing
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (hash, name) = l.trim().split_once("  ").unwrap_or_else(|| panic!("a malformed line in the listing: {l}"));
            (name, hash)
        })
        .collect();
    let mut changed: Vec<String> = Vec::new();
    for (name, hash) in now {
        match recorded.iter().find(|(n, _)| n == name) {
            Some((_, was)) if was == hash => {}
            Some((_, was)) => changed.push(format!("{name}: {was} → {hash}")),
            None => changed.push(format!("{name}: not recorded → {hash}")),
        }
    }
    for (name, was) in &recorded {
        if !now.iter().any(|(n, _)| n == name) {
            changed.push(format!("{name}: {was} → no longer exported"));
        }
    }
    let listing: String = now.iter().map(|(name, hash)| format!("{hash}  {name}\n")).collect();
    assert!(changed.is_empty(), "{} exports changed:\n{}\n\nThe listing as it now stands:\n{listing}", changed.len(), changed.join("\n"));
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
    let t = [(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)];
    filled_mm(&mut s, "#22aa44", (150.0, 120.0), &[&t]);
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
    let t = [(0.0, 0.0), (30.0, 0.0), (30.0, 3.0), (16.5, 3.0), (16.5, 23.0), (13.5, 23.0), (13.5, 3.0), (0.0, 3.0)];
    filled_mm(&mut s, "#22aa44", (110.0, 20.0), &[&t]);
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
