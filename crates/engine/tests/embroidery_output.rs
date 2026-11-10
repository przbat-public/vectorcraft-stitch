//! What the embroidery export writes, pinned byte for byte: the sha256 of every export, in every
//! format (DST, TAP, T01, EXP, PES, PEC, JEF, VP3), of documents built here, of a test sheet on each
//! fabric preset and of the real designs' artwork.
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
//!   blocks' ends), the export's options away from their defaults, settings at every level
//!   (objects', the document's and an export's together), objects the walk expands whose
//!   strokes are sewn along their path, a path an effect reshapes too,
//!   entry and exit points the planner sews toward (checked met within the Check tab's 1 mm too),
//!   satin strokes sewn with their round, square and butt caps, and fills whose rows the document
//!   sets (across each shape's length, or at a number of degrees).
//! - A test sheet sewn on each fabric preset (`embroidery.setup {fabric}`), the default fabric's
//!   checked to be the sheet with no fabric, byte for byte.
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
//! Settings that set nothing a plan reads (what a newer version wrote, values set and then reset,
//! the default fabric) are pinned to change none of these exports
//! (`settings_that_change_nothing_change_no_export`).
//!
//! Only the hashes and summaries are kept here, never the files. They were recorded on macOS
//! (aarch64), computed with one maths library (`one_maths`, the `libm` crate in place of the
//! system's `sin`, `cos`, `pow`…), so that macOS, Linux and FreeBSD export the same bytes: the
//! planner calls those functions, whose last bit differs between the systems' own libraries, and
//! one bit can round a needle point to the next 0.1 mm. On another platform (Windows, where the
//! system's library is used) a hash may differ with no change to the code, and an export passes
//! when its summary is within that much of the recorded one.
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

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
mod one_maths;

/// The documents built here ([`documents`]), as `sha256sum` lists files: the hash, two spaces, the
/// document's name and the format's extension.
const SYNTHETIC: &str = "
6224c12bde2eb89eb3ee87153b3fc3dabba42be26e55b9f3c3cbfd28251258aa  fills.dst
ff83847ac1765d73f2c3279451af4e0802da88a000f321483438b6277b888dd7  fills.tap
ff83847ac1765d73f2c3279451af4e0802da88a000f321483438b6277b888dd7  fills.t01
b494cc716612df5c27df680bc790a964e4dd555559df8b9c002107a2ec597a64  fills.exp
d03e603ddcfc760d249a5cbfb9b8727a98b6990c389f31f97dd1c2b7fccf85a6  fills.pes
7b66b6049360fd8334c450b2179ae1df10ec24a77017167464c6b4a4d5928d54  fills.pec
ae3e5796cba4e18b0aceebc2bd3c13e2e5be3e024b4c535b36a34cdfc5ab599c  fills.jef
74428520a672d1b84eb5d9fcdc85ccaa219adbf8515c602ed11d6d3ed97df4f2  fills.vp3
39797e76b30fbc5590344f74d78f56d2df5650b568fa19e4524b51e9bf735a81  strokes.dst
f836d6d6fd8511fad23882411900ce7fd44c47dd9f9f89afb7735bc21c51aaa4  strokes.tap
f836d6d6fd8511fad23882411900ce7fd44c47dd9f9f89afb7735bc21c51aaa4  strokes.t01
eb533088cadf0e803acb15d5346d50cb1689df0c0a95b151ca3072c5c713cdd0  strokes.exp
36a596ccfaf7b525a17aa7c501657b86dd6a54a6e073e02c50bf3c97a3290703  strokes.pes
85d22fedc8cb41e60c101a3950bfbd9961ade8f731b08b227988123ab7a6a197  strokes.pec
89335107460998a8cc4a8d1ce76ea5b47dad41a5602e00b323e2e8a2bb68181a  strokes.jef
88d25ea4c0d4a14088d77d6557ecc70181aef075d591b7add39ea17a5f015e17  strokes.vp3
8ae7bb241f3a1e262089abebb73a8db9158f5a7e3373d63dd6f0cafce4dcd406  compound-path-with-a-hole.dst
6d0a0922cf70c2d1c66a79d3bc6e60d83b3ab619d534fb2feb8a21a85573f3a8  compound-path-with-a-hole.tap
6d0a0922cf70c2d1c66a79d3bc6e60d83b3ab619d534fb2feb8a21a85573f3a8  compound-path-with-a-hole.t01
db6f4a76ad7074556c12978e9cfe05e944b5347cc8d56c7c4e3ee027dcb1bdd4  compound-path-with-a-hole.exp
5888e9626963920d452306e3c79e180c7b153d15715a73f53df17a9cd8ebd39d  compound-path-with-a-hole.pes
a5b7023af8ebbd73644828303e02115e52c64c5f77567027e9cebf3532248976  compound-path-with-a-hole.pec
2f8bc15a180965056ff7eb5997037259b74b9c622713fb4a4a93b133c1296765  compound-path-with-a-hole.jef
533f72286ca55498d2dc5cd290c40eb52ebfc7688427fdb1053f7ddc248d9ccc  compound-path-with-a-hole.vp3
bc190174a07e5924048bc1088722d7bce294f3fa3a50f0d7467b37c33da5d0b6  clip-groups.dst
3bc4b5e03b384d23220593654c1c8641cd32ea78bc740b4c4cea2b51df49eaf8  clip-groups.tap
3bc4b5e03b384d23220593654c1c8641cd32ea78bc740b4c4cea2b51df49eaf8  clip-groups.t01
af1efe86ce72786c20297eecd76880a5669af1af570aa1dd39d5c2f522baebaa  clip-groups.exp
1df42c74fa060d4da1ecc7b9a622acc81af7f0bf74ffa09397605ce43fccafc0  clip-groups.pes
abd40f47341c568902bd65dc89796f24344fa0477a5d26c59fe7937c843779fe  clip-groups.pec
21304b19fb02ee2a9dbeb8ed2b0390165cb66b63b23d43bb6d11054d5d08b114  clip-groups.jef
5aa2aa15e8561a7fc5d136ec3d7f84ceb1d6eb2044204778073941f9b0f33c1e  clip-groups.vp3
3f3b83a0b1f4193b2199ac29d05ffb1d1ec97564960681dddf6cf4de11fd2179  symbols.dst
6863972d02507e26c308013c6b569a2269fac6b0efe1ddd7fd37c1d7a30dc706  symbols.tap
6863972d02507e26c308013c6b569a2269fac6b0efe1ddd7fd37c1d7a30dc706  symbols.t01
962f2c9cc7dc263b69045f9aeb914ec40db87c85c64151170c8bf42195850564  symbols.exp
cfe9991453a50c6a15eff454b1ffb3b6bf3fd71cf1a57ddcc5f51f29d4c75bd8  symbols.pes
81faf61c9dfe758350d4b47c82e2b74e9e5ae8439f0c573a1b823539acf26b57  symbols.pec
3718226c628e11765fa64f7a5a440c531b63e1ea8088d306a0d5b7c373ae5251  symbols.jef
c3ef5c2eec96164fec5adba8292acb894213d5d456b27e1476dfa1f31de8a7c0  symbols.vp3
c104bf28927049c0cf5e73c214eccb55df71268a5b42c57b129632d1f307cf93  trim-after.dst
29927e6ae7fef7fa5576df926e6cc2654a5638274893533c54379839f28f0f18  trim-after.tap
29927e6ae7fef7fa5576df926e6cc2654a5638274893533c54379839f28f0f18  trim-after.t01
390ed7f8367c719dce416eda7fda62f3fb5c9655a00908ed1a0daad6832d47f1  trim-after.exp
2624bdc13efc3429c7affb955394be2f3f65da1d1968ef8b49da115c8e347418  trim-after.pes
91b83500a35ba460871e179aa0ef9f26c0a38f572ce4ef01c344988f61f9a0f6  trim-after.pec
5f6fd370fc59eae8bc53da70a96abc1b9f172d676fa13b73fbef14d450228bfd  trim-after.jef
710c6703db958c2410de758051c1174382af631eb256f2b7f27d9b74c7239480  trim-after.vp3
55d085fed687553aa2a2954eadc8faf18283ad1e23fc24d9e4691d0de810caba  opened-design.dst
1d4e93001c32bd4f038b8ed0e9d7379ddc6fefc6eaad9734dacb2d6866b981e8  opened-design.tap
1d4e93001c32bd4f038b8ed0e9d7379ddc6fefc6eaad9734dacb2d6866b981e8  opened-design.t01
a5a2c406b82afe1e4498a9e5af83777ea6d1cff98cd4c1a08839dfa694187bb6  opened-design.exp
6d514444082e60c38595eb38fec6fc591bd1113b44977b3ec8a95bba9c132ffc  opened-design.pes
af9a971ab2296ec2ecfdf1a566e33a983bb4288e54af734337c0993366b84475  opened-design.pec
26c7a6aaca9b0eb3ca0037bd92cca6e78baf47d2a74b40139f72e247dd909cc3  opened-design.jef
c036376ef06f0d4860fc2f92515242faf4cbf2d6f7f95a003524baeb80853478  opened-design.vp3
d79931d563ba3260d85869e32a960332af7d26b83b8408028e55c99e7b6c702d  opened-design-clipped.dst
e798ed4def99f5270d84dc276f5dd5f2bc0f486f8257e338462bd238635fe310  opened-design-clipped.tap
e798ed4def99f5270d84dc276f5dd5f2bc0f486f8257e338462bd238635fe310  opened-design-clipped.t01
08e630497ef727f246a030895c0586ee4f21f107d0dc9dc006263c6658236ced  opened-design-clipped.exp
6e382631b5cfdc0b01d5e90c980371d5fc395bf658f013b718b5c6362e19a454  opened-design-clipped.pes
4fb3978275762737c567004b92f84615ab87d3266b3daa03c9d965d570968808  opened-design-clipped.pec
329bcf71915fcfcd043e965e16cd05f835a0ed860925f76b1c8410a63fd2c819  opened-design-clipped.jef
d5eeb44fa89713ab78017866574aa9a3a13358f95c7f79fee33a8779647d05d3  opened-design-clipped.vp3
1ace1f9dbbdc537259cb8d5ca2d7b07fc3ef466500fcb16850a501cbed5077e3  opened-design-cut-at-block-ends.dst
161d7a8936350ac80062705f8cea3063edaeab6ceb7f8c58ba559a19a54c0ab0  opened-design-cut-at-block-ends.tap
161d7a8936350ac80062705f8cea3063edaeab6ceb7f8c58ba559a19a54c0ab0  opened-design-cut-at-block-ends.t01
7d4ca828ba79b421253b2c1135e43b71244765dab2e5e07eced148956ddf2671  opened-design-cut-at-block-ends.exp
4ed60ff9f8e8ff7339fd13a4a946fce01c18a7e80f481267c0331f806d87c201  opened-design-cut-at-block-ends.pes
6da95aaf2eacbbeddc5e4c3ca41376c360fbc3b842ce6617d2c4321333ffe527  opened-design-cut-at-block-ends.pec
2c95dbb97edb623dfa6d1d6b7163ed2f04e8d5132b9c509efaa0dfe287c3941b  opened-design-cut-at-block-ends.jef
04cca51b1f4ab2431c82fd5b34a41225acb9267b78762940b4e2ceb59ada014c  opened-design-cut-at-block-ends.vp3
9b9f64285464939192e1076c003c6f053ff552e164070e7d32968b336ad84c76  mixed-stitch-style-fill.dst
61d75dc86254af3d8956106a7948765633ddfc33aa9d6230ec6eb7f29835d659  mixed-stitch-style-fill.tap
61d75dc86254af3d8956106a7948765633ddfc33aa9d6230ec6eb7f29835d659  mixed-stitch-style-fill.t01
f84b360f60a44feeea19ad1ff41f344d257ee27c335b77a15aa9b8444668cabb  mixed-stitch-style-fill.exp
2bbedf4d85f4a7e56e1ff2b4dedebb975794ef482b700ecfa0b6585b1c6cf914  mixed-stitch-style-fill.pes
92119609c42789cdfceb298a7c856cf42a1a248b7cdab7c5f3721e521fa17145  mixed-stitch-style-fill.pec
399ddedd55f7e73976a1917f724785715301f14b1cf47dd716ad295f84cfef57  mixed-stitch-style-fill.jef
b230e3dd22f4547a4d21fea504edf59ba75b3aba87a99df6236a435b08f66e51  mixed-stitch-style-fill.vp3
6c8c082a8b6d8e6809a02d53fa7d7d4d627f76b443301d33e9fb0d96bd935532  mixed-stitch-style-satin.dst
d33cdd3dd6f0cfe4236546a15259889a16b0fa797b4e6ec268445ff63795a6a6  mixed-stitch-style-satin.tap
d33cdd3dd6f0cfe4236546a15259889a16b0fa797b4e6ec268445ff63795a6a6  mixed-stitch-style-satin.t01
1605e7dd6f57e5a6bc2383e99283d021316121adcb4658dab314598b0f9b939f  mixed-stitch-style-satin.exp
538e794f1da73f5a430e724d0fbd11a4636cd9fdcdbfcbff2819bed15ad82b1e  mixed-stitch-style-satin.pes
c42d730d7af599da0023f4793fe0274a5bd49ae65b0adc90af77cac7fea61d0f  mixed-stitch-style-satin.pec
eaee935976be194416276120e87a492a3ac283f45c6279f27c70b29e92f763c1  mixed-stitch-style-satin.jef
72e7699f1d5d25a5ed3716b85991bf3faea43844c5b15837de658d524571e6b9  mixed-stitch-style-satin.vp3
1eafce568d27a982c416ef59cb1d89f085ae0f2cfc2ba94c71399d8cd5a5c0a4  mixed-stitch-style-running.dst
0d1a7534cdc23ab1e0ee259273fc11da46a790ed03cc7ad31a9797f5d67bf904  mixed-stitch-style-running.tap
0d1a7534cdc23ab1e0ee259273fc11da46a790ed03cc7ad31a9797f5d67bf904  mixed-stitch-style-running.t01
643757e8783d69a2b2fc0d3d5e340b431dbf40a9691e201555ca721421b73dcf  mixed-stitch-style-running.exp
1db76897959188959fce0e009cd8124c2af9c6f7442df576dd027407bdc886ce  mixed-stitch-style-running.pes
bcb015f33e25490c91b0f0478be6d78f5d4abe0d1f5888f9c653b5f2058e9e64  mixed-stitch-style-running.pec
ca1adee7a0404173151e9e990db31da999e2457bb3c3a81df567f56a63df456c  mixed-stitch-style-running.jef
93bc1b19ecdd85b4532efcfbd77029f8e489dac9e6396dd32a88fe342a98b83d  mixed-stitch-style-running.vp3
233070a6889b2e93eec90934714681982139bcdcd7af47d00b1702739067185b  mixed-every-option.dst
1170588cf3682b413e1495e337fccc65185ca9162324cda808b9e2db4fb7f0d1  mixed-every-option.tap
1170588cf3682b413e1495e337fccc65185ca9162324cda808b9e2db4fb7f0d1  mixed-every-option.t01
438fce34ff922af054de30f385cae4e3f927ca9f0e8af6b9e6bef073a20e28f0  mixed-every-option.exp
abcda3b55ed25b1ce80033496737f2cd1cf948758769ec6596d145bc9c75cf41  mixed-every-option.pes
e50f40c1be8d0c74f9ac88dcdf8799ddf6500524efa36904049d9f43589f70e4  mixed-every-option.pec
40734acd42f09b45f298c736b5ef458b22c965f716409f6cdacc76bedfc67327  mixed-every-option.jef
3846774866961a52cc6a446390fa0b90e7e58db5979ee83ae77fc67d976852d0  mixed-every-option.vp3
3352e47b6ace841c2be502c25d1ba1e41683cae463da4f65b9c4b837c49ad3cc  mixed-shrunk.dst
2845bff2e90436a2d5256ce7ecec6d69c1cc4a3abb63fe029030f0f634d0469b  mixed-shrunk.tap
2845bff2e90436a2d5256ce7ecec6d69c1cc4a3abb63fe029030f0f634d0469b  mixed-shrunk.t01
a2a8e3c4c4fbc0f58a97935a68c6021b0b4d7fe06272314324871c9f72e7c508  mixed-shrunk.exp
56c9c6d398d91ebfdab59bd4039d93cd8d37da16860b22145cf8f2d96aed8bc9  mixed-shrunk.pes
8270de7dcdbadb45d2543390083a921d5ef2e554c2c699ada4915feeea52570d  mixed-shrunk.pec
2cf3a545f0f3fe97b7e3e9d6c1e91a13311483c8ffd5b52313059a480a18072b  mixed-shrunk.jef
7797d069a9b451cad3a1a45b1fcda0ba30803780959b402dad16f5f3a7ed943f  mixed-shrunk.vp3
6e6cf70ef515a7224d1f00f7b33c181ebd22bcf320d05413157daaee802a61e0  settings.dst
620d9ed9f5c560b3a2416db2f2e953cbef3471a9ae852052d13cbe540391cf93  settings.tap
620d9ed9f5c560b3a2416db2f2e953cbef3471a9ae852052d13cbe540391cf93  settings.t01
dfb2cf45c0dde8a80153c945325bfd79dc9fbeb700fe6ca44d6fab26c1a40090  settings.exp
51e6455e38d993305046f94ce0a1d25fad2e33a6f3b023c7c999bb1d26895399  settings.pes
0d279cd3fc116502d7573758833b460b89fd2a40943a2e90493f8a3ad4630e8b  settings.pec
b143adf315ce8157b8276bbf346e0580739c277bdc32d856666ee42702e03488  settings.jef
b5b2ae8997e75b2ae64853a01b886a0796f0ae49e4dc1f5143d79d84180f61c6  settings.vp3
b42e82e03518d7e292a25033e860ee675eb08da33665df840caddc3763b01b9d  expanded-strokes.dst
14e096e68a01a176afcd7cd2fb298bb8cbe4b2037d66fa2d6a882343fd0ca614  expanded-strokes.tap
14e096e68a01a176afcd7cd2fb298bb8cbe4b2037d66fa2d6a882343fd0ca614  expanded-strokes.t01
3fd2b58e4d10a6b3a9e6ae4eacdb7a0c0d23884431b5af0e671b89fa07dff473  expanded-strokes.exp
418630aa2248ea6d9143da6e6390a61c98e26359fbc225e65178a021f6cec9b8  expanded-strokes.pes
cd1a8f60335f669eb3fcf80440f358cc5d64ae760404c7e5265c632f97e6c57c  expanded-strokes.pec
58ef6e71acd8913fad552de67af2ec7ec5cda7feeb94874b672d61e679b4106e  expanded-strokes.jef
2e15f1d69abb15a12747e410e84f17d933325a1f4a48c8f2c822dfb2824c3dc2  expanded-strokes.vp3
fce9d310b0ada80d79c9a257ad2556c59193f266e2e67e42f3ea9799f567d8ec  exits.dst
6288fa0e2280346aed1d614e3d39c286b3c1afc5b2688971d1f440ade5676008  exits.tap
6288fa0e2280346aed1d614e3d39c286b3c1afc5b2688971d1f440ade5676008  exits.t01
e18eeebc37bfc4058e77562b175095c25910d28d31b7e93c0f368a0a98354958  exits.exp
479cff290a256393a5000370e86cda9302399dc90ec99b4dc2649a5df6fd45f1  exits.pes
c44621c4ff83e459eaead14246d6e501a32eaf458fb2fff5986470214e71c6cc  exits.pec
2ccd0a2f00269a6b034aa696f908233559f7c75c3c1b13e42d6a223cec02fc01  exits.jef
b20992b7856d058115f889b35679dfcbeb0b683a306da77cb64ee516b91281f7  exits.vp3
9b7f1522c65324c21d8032e7d82e498fa296844d30f16a32d213965fd36bd0f1  stops.dst
49d5a6c977f5baaef896bc1cd26879c467b02880bcb4a1ff3de5f47a05bb88f8  stops.tap
49d5a6c977f5baaef896bc1cd26879c467b02880bcb4a1ff3de5f47a05bb88f8  stops.t01
704861e647dcaadaf9a8e544124897272f92d7815c47f8770b1e5d24eac5668e  stops.exp
99ef9afbf210186a494d977404bd600bb8ab15dbb96d8f4ed585b5e4d4ba6a61  stops.pes
181d9ac9fc9d6c34d7f776e58dc502dc98d61b2c25f589383d779d2041364431  stops.pec
d2238d55ce687e38e34bda4876c983f1b21034c978580abc19e3b61dc48259bc  stops.jef
a77bc0e2ff5f19ceb4bea099c075139bab412a8c1cf02da2759b55e301d16acf  stops.vp3
170de223979a965509b03984e8e64dccb9d92e02ae781e87605591cf2d0285c7  capped-strokes.dst
533a712e12ba9842941a6e67f870ef1024a665da9a9f3a5e8b660ebfcf0cd5d6  capped-strokes.tap
533a712e12ba9842941a6e67f870ef1024a665da9a9f3a5e8b660ebfcf0cd5d6  capped-strokes.t01
d6370e91dfed18ac02e02cab146d31e2320bada721ef8af8488461009ee223f4  capped-strokes.exp
1a67cb7368a5944887a67f50a55ec65e31d199ead766e873670a86dda6015050  capped-strokes.pes
64e2542d2aa4b564731d42d2c70be2a349561f2179e93326cd6adb20ff61ac39  capped-strokes.pec
a386695a2e80cd6f33720a5fa476f640c39fffd61795e570324ce3f3b3edfc89  capped-strokes.jef
6104907b81adc10c0d84c38266d04e9f6a4de43c441cedf874bc1924fc2a4cc3  capped-strokes.vp3
f4d0a71ecb132a1124927ea602c958e1011938419b361ad2467bb32c5271acfe  rows-across-shapes.dst
662ffd20da2ebe3310762116a4147e66f1ad05d4bc2ac53ca55a002acd3b368f  rows-across-shapes.tap
662ffd20da2ebe3310762116a4147e66f1ad05d4bc2ac53ca55a002acd3b368f  rows-across-shapes.t01
e63d6b220809428db25ff80ba6ce95391473d8b41597799b7f20dd40a02edf88  rows-across-shapes.exp
8c97a2d3264f0cd11272afb4aabdaf61dbb2ca78a186cca0b2e182bca1929bcd  rows-across-shapes.pes
1d06c7cf947659da34dde20c48042fab62307f08797c79298ecf58103a10f17b  rows-across-shapes.pec
4bf8f19c4bfcb0a99c337769ba3a3b9fbffa60ac6cab785b708ac337d9a90fab  rows-across-shapes.jef
510628a2a4a8dad339a025bcb1e24d393e80137cc80a691617bcf8793d8becd5  rows-across-shapes.vp3
ec01901dc314e31ea2dc2cd117ea33811ce0a6ff03b68d0661c2e90aa33712d9  rows-at-30.dst
7ded99ec7c3ea29e6cb45087108c228b07d155514b66c12836cd493a55cdb262  rows-at-30.tap
7ded99ec7c3ea29e6cb45087108c228b07d155514b66c12836cd493a55cdb262  rows-at-30.t01
3e2611602a015fa329906717863e4e12322d4a7e5bdd71baaa170e7b73ad77f0  rows-at-30.exp
d567e9f40b124619e9e17ff2dd5e6aaf886ba9ae912ad8e7990a548b036e2374  rows-at-30.pes
74d54c968e2d650f2ca7131377e38f354dfa9065a219998b019d689d8ac7c8e1  rows-at-30.pec
69aca74622e01ae1df761ebd84d66976e6f50495c371684eeed155b924d2b6b5  rows-at-30.jef
82e68655d2c131cdef610deb899a9a5cd662e95e47f4a75af6e3fdadaddbbdaa  rows-at-30.vp3
";

/// The real designs' artwork at the person's size ([`DESIGNS`]), listed as [`SYNTHETIC`] is.
const ARTWORK: &str = "
e6433c30dd38b74a9718d3f7d9868eec657e08cc3b323322f3301a31042f0dee  bullseye-twemoji.dst
42a002c7eb1c1fa25142d2ed6e0580a6d31b8f289fae2f2a256de6756644976b  bullseye-twemoji.tap
42a002c7eb1c1fa25142d2ed6e0580a6d31b8f289fae2f2a256de6756644976b  bullseye-twemoji.t01
d82280a48e61ffe590603bba51a5e5bf327b26ab7dca6aeac71dc4633e62eb92  bullseye-twemoji.exp
0569c0cee6127878ca1ffd580c1cdf4de397cf2489d5842832540a0336dbb1d7  bullseye-twemoji.pes
48be90a7dcc75b0c594a62dc1bcc7b9fde19bc40cf7f432c1da5e47f89a9cb31  bullseye-twemoji.pec
5b9152276cb510a3b37140abd6892c7734b2803b9b4642a033433c2b1b6a89e7  bullseye-twemoji.jef
9a259fe5233e602c7fbc1670b9c1722f970077160d12909c40b8a62783e8a706  bullseye-twemoji.vp3
d4937fa9c5de14d64ab85f4019d242f735701008b5d56bedc5db5ef2306e3b89  fox-twemoji.clipart.dst
604a384ba844782378e2b359fa595bb95fc612e45c270ba5cc77766a1155de46  fox-twemoji.clipart.tap
604a384ba844782378e2b359fa595bb95fc612e45c270ba5cc77766a1155de46  fox-twemoji.clipart.t01
5d3d34ab1944bb0f6900e414554c5de5d5e5d633c1ceb3f5b1e859340480ddb5  fox-twemoji.clipart.exp
a7375cb7b5f6696e5e8e15681685676dd57c8b07ffc916782c9f56a4906a4f87  fox-twemoji.clipart.pes
043b0eec2fefd55531f5977be63f355069ee00a4197164a9cdb56f917cbb6143  fox-twemoji.clipart.pec
b50f50c4acad59bf1d3c2c2d233ac0b0971b284fab2bf4b749412215d07ce10b  fox-twemoji.clipart.jef
35cf997d568783285e55dc9cca38a3316dfd2505f5c54a3c468251626b1aaad4  fox-twemoji.clipart.vp3
2634d0052f219d9c0c9c9a1d9b96a79e3ef88f020681fa71b65533b7a1fc1f09  heart-suit-twemoji.clipart.dst
59fdee8544e2cebe588b5863bd98c8bb5066230d1f7d0d62c119d22c88ef3876  heart-suit-twemoji.clipart.tap
59fdee8544e2cebe588b5863bd98c8bb5066230d1f7d0d62c119d22c88ef3876  heart-suit-twemoji.clipart.t01
9c3e097ddbd58c4b162d03749f831803dc2765b52887884155d38ce45fcf9999  heart-suit-twemoji.clipart.exp
af02492714fde8d280b1e45f86f12ec66ddc6fcc32fc747830c58d650eeaeab7  heart-suit-twemoji.clipart.pes
1d0bb3ba4c7b3c43ece2b32549023306750e6ede4bc43ee43fda597142f19e8d  heart-suit-twemoji.clipart.pec
4d7059dc6c4844d8fa4595f753fa2dfc8fc6793235c3c934abcdd1b06de95c69  heart-suit-twemoji.clipart.jef
b455a91a4b0c85cd414bf998f15ecc3a5537cd666e6daa13ce2e95596bd5e53d  heart-suit-twemoji.clipart.vp3
874a133cbf76bd02cf1d9f5a8701f1d3e351fa3592d1f1558f5db40305c256fa  joystick-twemoji.dst
ae957e8b323136ad4518206663173e3c08d2ee4ae948897de0f8ed2cb4b394ee  joystick-twemoji.tap
ae957e8b323136ad4518206663173e3c08d2ee4ae948897de0f8ed2cb4b394ee  joystick-twemoji.t01
a32b8d492138a936b25e193030d7bfda02c1ac51d8089c493647fd1328c41c62  joystick-twemoji.exp
470f0d1348750c45cd7be5ba6b9e1d44df150c3ae71856c7c9db9e00fabc2850  joystick-twemoji.pes
ad65bc0e09a9220333152e7598edb60c6cc554d73f5d08d94ab8d282cad9c30b  joystick-twemoji.pec
79f0068778ddc1eaf867b6a00e6ac5a1a533788e8417643d2fe346d8dcc55359  joystick-twemoji.jef
4bf099f2cab6db7be9d86405417faf512f669bcd1ba49042322060ff884ac0b0  joystick-twemoji.vp3
b9527d81cd13ef703652cee272b888af3eea7f5e240ecde64c0057701e300253  pool-8-ball-twemoji.dst
1af4d009130b50df6f932666e28eba4e37e8759523fc3f417f6d218e47730b58  pool-8-ball-twemoji.tap
1af4d009130b50df6f932666e28eba4e37e8759523fc3f417f6d218e47730b58  pool-8-ball-twemoji.t01
3c92a08dd36a6b9620db870fb6bb7409bb9e130a4d4dd2d00ac76e6699a7298a  pool-8-ball-twemoji.exp
4cdd304aa5897988338e5285eb4d193018f51d930db4cbcc438fac9fedca9199  pool-8-ball-twemoji.pes
478dffec9c5cfb1c851abc20f5cbbd2f765c185392c05090bac5a0f0f0739450  pool-8-ball-twemoji.pec
9a398d26a327e66c3a6fce4a9675d25e290e8a0ff422e78e037eb081f186601e  pool-8-ball-twemoji.jef
14bdecaea46aa1f5f378c898acc510135e640c418687e64c5fc4dd9dd2c5b9ea  pool-8-ball-twemoji.vp3
e382b7227690097b2f736c9dc3990f324915a830b57ff23a05213f2581f19d72  puzzle-piece-twemoji.dst
cfa7411fec258426e3847cc037272ab54e69f6e0fae46f85370bf04ad7234dc1  puzzle-piece-twemoji.tap
cfa7411fec258426e3847cc037272ab54e69f6e0fae46f85370bf04ad7234dc1  puzzle-piece-twemoji.t01
9218a1fc72f30d87d509bfd67772b0e49c484052ff2e295e97d6c73938f5fbb6  puzzle-piece-twemoji.exp
95c0c890917c6d78da692131263e9e0ac5ee976b50cb9b6688247cf0654252f4  puzzle-piece-twemoji.pes
f722b93f318c26661b7d3dc3375c137a4f50aa648b24ce950eefbe2ed19406b5  puzzle-piece-twemoji.pec
2a89ece070484f0c7c1140325fcff0cd55bd984bd7d6077e28b3453f4cc0434a  puzzle-piece-twemoji.jef
7b883aa127b179f0467f17e27551981e535a30fed597ab3f017a39fc7f3e6e17  puzzle-piece-twemoji.vp3
96176b38b53975e382d7d47bf2fffaae4425d90b2e7e1f8862e2499301944f3c  smiling-face-with-smiling-eyes-twemoji.clipart.dst
5be2fee15bd85b2dc7acf793004fc3f9ee07052b83334abf3b7dc65f2557283b  smiling-face-with-smiling-eyes-twemoji.clipart.tap
5be2fee15bd85b2dc7acf793004fc3f9ee07052b83334abf3b7dc65f2557283b  smiling-face-with-smiling-eyes-twemoji.clipart.t01
aed7b8e9c42eeff2147db9040b4420725bc70c186d930bedfc71e6c6be46dfa6  smiling-face-with-smiling-eyes-twemoji.clipart.exp
4e066f93172d6275ebd7e17501cc3b1dd0c65e151f02cb34ac7b54356de94dc6  smiling-face-with-smiling-eyes-twemoji.clipart.pes
37bfc2dd232f0def579677625e179268a14173925b50d35b51a1c44879862b36  smiling-face-with-smiling-eyes-twemoji.clipart.pec
43bc73e64fd4200cb59817f6bb36393c62cc527ecf5a80fe9549c33404d7f07f  smiling-face-with-smiling-eyes-twemoji.clipart.jef
fce4b53f63b6767782b537f662fe3132b452a8008d1659bc21faf02b2497c880  smiling-face-with-smiling-eyes-twemoji.clipart.vp3
d47ec92bb0edd855e49f7b645558e92eaa8bf92089dcaa2e62f86ee05a98318b  sos-button-fluent.clipart.dst
7654335b2ccbcc764ed8ef76fa1555c27fba2b509b7212ec4902666bbd58bddf  sos-button-fluent.clipart.tap
7654335b2ccbcc764ed8ef76fa1555c27fba2b509b7212ec4902666bbd58bddf  sos-button-fluent.clipart.t01
5a2cb9bf81b1988e773484429cd568af15f8e9fdeab6d1c8895b2f9e974e775d  sos-button-fluent.clipart.exp
28e11da6909334480d7681b0bbe501fb7fa6291cbf400487cf8cabd2df49a36e  sos-button-fluent.clipart.pes
0f1df47fa94486e8fc0e59c250121f96568592c7c89cbef489915d0f63921edb  sos-button-fluent.clipart.pec
4ada77cafe4d2df30a6cac5cfd8b6de808a382287144ac988c0dd12e698e8ef2  sos-button-fluent.clipart.jef
755ef73a6e49efc9c64c84bd352a3ac86c16dadba74b2374837ea22ccf2fdf2d  sos-button-fluent.clipart.vp3
7944c4b64e80811208a743c19af6bf7d8b6eef9ea2a39337caefa50c8e564b68  video-game-twemoji.dst
250e14f61917d45492c389ab24c4c741ba641373b896702898482ef052a9a45a  video-game-twemoji.tap
250e14f61917d45492c389ab24c4c741ba641373b896702898482ef052a9a45a  video-game-twemoji.t01
cab9f2a93e91c8505cb942c62245a3f9872335152c7bb07d47587dd29a597730  video-game-twemoji.exp
39a2618dbd6e7ecffd1cdd22ca35844213b766a8f54616ccbe3d4c58f33621f1  video-game-twemoji.pes
a70b3a87f516c2c4cf36b42db9abcb4da974cc436a220130cbdb46f805e630af  video-game-twemoji.pec
d9b9a2d463fb56da6ce80fca77bf35a2dba5e0c504e8f83fa58b65919db5f906  video-game-twemoji.jef
aac5884c28113a6eb147507c3cbc9393258e0b956ad9662748ea65ac89bea7a3  video-game-twemoji.vp3
700ec95d1ae06a7c44d4d5e0e6bbd931e1a6451281ce6367ca69139de3724002  watermelon-twemoji.clipart.dst
c8b37f3560fa842c4224b663fb85666ef18f19c0cb348e02ff8d7ffcb453f75b  watermelon-twemoji.clipart.tap
c8b37f3560fa842c4224b663fb85666ef18f19c0cb348e02ff8d7ffcb453f75b  watermelon-twemoji.clipart.t01
238e0cabb1c971bb917f87776d9f1954359bfb4b3d1e1036ac11b50d92422deb  watermelon-twemoji.clipart.exp
00abda206de8caa29b1bb8ffd9e0f952fe40d834543f8c2bdd502f6c21c0513c  watermelon-twemoji.clipart.pes
8f8140f4490f7a5cc261abbd64f3d05f019941f674674f5416cee287b334fa79  watermelon-twemoji.clipart.pec
f22df85d5dfa996b8e642692c7b80a115f083512250f609c29abe4f147edcbc9  watermelon-twemoji.clipart.jef
efa863ab97f6b056be8f0ea1d3300b1f205bc2ba61e602c37b4f46c969b674f6  watermelon-twemoji.clipart.vp3
faea28e9d25eafc401b020d3218b1b8398ac11192f4732cb1cdbc4e4276a3db5  zebra-twemoji.clipart.dst
3e3a32ed602ee1bfd63a54849c4b00f8a28ed816e749655145f621aee9b07747  zebra-twemoji.clipart.tap
3e3a32ed602ee1bfd63a54849c4b00f8a28ed816e749655145f621aee9b07747  zebra-twemoji.clipart.t01
edb25bc2f908aed3ec711abe41ac6d48ddf679303439175d150c2ef56b2625f2  zebra-twemoji.clipart.exp
cd58fd54b9ee4edae4e567137e8829f04225c782fe72b90bcf8ffae0d0b1f8f8  zebra-twemoji.clipart.pes
fe80bdb133eb509ffec36946ea0fc6261050ef003634c91c6047d9b3216a3451  zebra-twemoji.clipart.pec
50dd68cc9700a09d1c4cac2b60a1944d0783eeaa11240deda5294c99882c2c6c  zebra-twemoji.clipart.jef
2034dcb4448048d880e6623208fa67d1c3271ebf851f2ca9a08330a1e7d48836  zebra-twemoji.clipart.vp3
";

/// The PES, PEC and JEF exports of [`SYNTHETIC`] apart from their thread numbers ([`threads_apart`]):
/// each one's hash with its thread numbers blanked, its name, and the numbers, in sewing order.
const SYNTHETIC_THREADS: &str = "
2a0bc9d2784dddbb312d03d259f432691818d5292577b8350bfad30aecb1a52a  fills.pes  5 21 37 5
489ac177e2aed0eb06ddd27ef47bca98bcd7d104ffbca2aa5389d493e71f7a27  fills.pec  5 21 37 5
68c51aedeeab1c1e696bfa675f1d336567a1cd09245420fe8dbfb2618577ee88  fills.jef  33 59 45 33
c3a06ac48968baf33f01b1cef11bf68f9a1ee1d24f5ceb5bb3259e5522aa1bb8  strokes.pes  5 21 7 37 52 24
117af9ccdd3e6d987902d27c05235b4c30b29e1832d3951d491e31273103b074  strokes.pec  5 21 7 37 52 24
5d7230325fc878bdc57c4baa2be365a2062165cfe894e579b7a3edf4234b2aa0  strokes.jef  33 59 59 45 71 14
39283d5c63f9b6892771139fef9a07b42765f908d20e09253f95610929e2da40  compound-path-with-a-hole.pes  5 21
a1fb3d39add8cf9f0b3e25ee4551f4506a4b8cfbe5bbe43d99b71614abe4f4b3  compound-path-with-a-hole.pec  5 21
81a38c514cbf2521ed634cb170c94fb879d09b5a688a01174e5f1ed1f23aadf7  compound-path-with-a-hole.jef  33 59
2e2b88b6a20de24112a6671e8859100ae38e8964746740933fd5eda9ebd710f9  clip-groups.pes  58 5 21 37 23
831b9a803f3f32ee9c4741387c7d4647116e3611618b12c43047a38ae82c0b66  clip-groups.pec  58 5 21 37 23
eaeb3aaa218c84167928ddfe1a12df2ea91e216e244f49bf17d4d7c24c9d8d4b  clip-groups.jef  37 33 59 45 1
59edd2e156ac66b6a0e5beff6bc98b84bb83ad0b34f11217f4557b82695f9f3f  symbols.pes  5 21
407df9dda759e2168959193cf472bf17e11d99d7e82c3215f5b5e0ecfb69c7bf  symbols.pec  5 21
0facfa57dd2b8e5721d9ba1e4e9699a684bf1818f1295f06a51181ba121db107  symbols.jef  33 59
7fceb18be0ed8a66f66fa71836f895dc8f74b9b34bf7abe38e9faaf99ccc7d2b  trim-after.pes  5
7367b99fdd86788e64b85a6807e3ffd4a90ade27e1776958f753c36ade539539  trim-after.pec  5
204b562a67b023a6a5840a990e0fb97a27d80ba09c646bf1c74310f3af393451  trim-after.jef  33
5637d71112f7093b50cb7614427a715115ab449189ad47f053a147498e5f44e6  opened-design.pes  5 21
1c611f25ba51a104e3f6eff71fc161db9934590df650e721f77b2c2e188a28bb  opened-design.pec  5 21
5b5bdd7f89c3586f681783f4d6000be4f25fb651764b2e30ee9cf2692e4d0231  opened-design.jef  33 12
9036cf5c149bd58c818780f8ea406eb0c75c7a7c6b8e479dbe71d6d7f50ece32  opened-design-clipped.pes  5
cd364ffc2f450c3fb20324c5043905e034558351078bae943992a2f1f8aee72b  opened-design-clipped.pec  5
49860d6b1c22952db3d338e262fc7994b3a069c8cdcdd31ba7e7aa30eb9fa6fe  opened-design-clipped.jef  33
5774353ebd951556d479a1ca527ba108ee316337724e07c96c7c6f31a9879f5b  opened-design-cut-at-block-ends.pes  5 55
41afd61150b4a1c95a64ab19d045d90b91c398cbfc7b7e7417f61d1a222f2b1d  opened-design-cut-at-block-ends.pec  5 55
d94eb984a5171bdb88240ddc94120244f53cef38d50df07ebc523becb9c17127  opened-design-cut-at-block-ends.jef  33 59
2f6b5af084144fdabd01102c1940fcdae4486aed031b47552c86610956718145  mixed-stitch-style-fill.pes  5 37 21 7 52 24
ad8cacc8b006227210edd00f8d1377835b5de1471aea3ac9fd89bb876f745d62  mixed-stitch-style-fill.pec  5 37 21 7 52 24
690c5922a64b286ccdf47bef09b2df7d00bf70a2940ba29746c116324908aafa  mixed-stitch-style-fill.jef  33 45 59 59 71 14
68404f49e216663779cda4a49cc079fee4a22421755173c3d73428a874324419  mixed-stitch-style-satin.pes  5 37 21 7 52 24
f0575ed808b419cb6322e8d683c76a00d58805f10db411581e07bdb15c7c5c96  mixed-stitch-style-satin.pec  5 37 21 7 52 24
a3a929d81ec9d123764ce19c7068089c96fa17e3da7071048966cb6c7288ce42  mixed-stitch-style-satin.jef  33 45 59 59 71 14
de0d2f4e0cd32f5392dfe444a2a7c3c8b3db8e9d12b2e3cdc2335638fed426a5  mixed-stitch-style-running.pes  5 37 21 7 52 24
3274e196b0a24188b69ebce599cce244e08172bc663e96ae82356dce542b1879  mixed-stitch-style-running.pec  5 37 21 7 52 24
e74b5ee8658196e1174f45bd5b04c4595adcc81e09992a4a1450d6b508de6a16  mixed-stitch-style-running.jef  33 45 59 59 71 14
c38e65e5eb530c8fb470e5baef4fd9e762e84ffe39bb3399dc0e6cf6b013f02f  mixed-every-option.pes  5 37 21 7 52 24
ec17c33f176b4a9da456a6a9f21f63990209cc09e933689433359f0420ee968c  mixed-every-option.pec  5 37 21 7 52 24
e382d7a5b85d40faba1c384b99e425a36268ae2a3d817afa17e5a8bdfb161ac3  mixed-every-option.jef  33 45 59 59 71 14
d8c1b3d073c4acf2671ecfc53143ff483dbb6d4f5c5ebefb96c4e78717580b01  mixed-shrunk.pes  5 37 21 7 52 24
4ca1a05ed8ba5545d45923c50ab06b133d8177cd5481075683ef5163cecca76f  mixed-shrunk.pec  5 37 21 7 52 24
6335b5fa327f42b0754f00f0269f16815a75bc593d852b04eba71a37eaadd30e  mixed-shrunk.jef  33 45 59 59 71 14
48ad2599d779548dad86bd963a17d94e2fd3df7f95e0e599c23d92f8a4df3d6e  settings.pes  5 37 21 7 24
acc6a2a07c8f4117a29347227e6240ecf83fb4f5ec00c506e72b7f88e594a23d  settings.pec  5 37 21 7 24
44ea303187ffce8b911b1e40001a2efe29f5028e5e9d0b85cf306b61c42a5e8c  settings.jef  33 45 59 59 14
48bd475614f42ce1cee0c44ec7c3d2f870ba3525523062d5e999938cdeb400c6  expanded-strokes.pes  5 21 37 7 52
0e164374b2cdd7a48299ca9d37f58f7c6239180bbe036d2b26aebebcf12013ad  expanded-strokes.pec  5 21 37 7 52
ae2d37233a3647e5f49db76c60744ad6486ad97bda52efcc76e53f65f0e2cc44  expanded-strokes.jef  33 59 45 59 71
7525e937850f077b6d72fbd7ae897b4694f12797d2d8c0c9b51b8d320b3ddcb6  exits.pes  5 37 21 7
61f7c43d42260f042d6037004bb38dd644e2806f020117671d842cd9abdfca8e  exits.pec  5 37 21 7
a3e4b067d744441d2a8c4ddeef48373d866aebe41326aade0c20252f59ee69d1  exits.jef  33 45 59 59
0cbdbeaf8f5a6b54c08931ca2b8a8132f27e10b2ee67498351caa446d8ed9915  stops.pes  5 5 55 5 5 21 56 56 56
0de51589ee049991fc511ff98f549cb9742893c818db4450209a826ac44b9043  stops.pec  5 5 55 5 5 21 56 56 56
e4484bd08cdd4abbc709e8285797662885f60e3f1eff64d4ce448f1ec03db8cd  stops.jef  33 33 59 33 33 59 75 75 75
99403da4047f7c9032a850e990b4e8da0bbdf1cb02d08419cbbd55752e399f44  capped-strokes.pes  5 21 37 7 52 24
1d18e3d39ab1bec96c2a627a669e619a2fd6367cf3506bde399680ba0075aad8  capped-strokes.pec  5 21 37 7 52 24
efb28864ad241ea1cbde14c1d84b344e00c88d209b6297ec3f3bb418e168251e  capped-strokes.jef  33 59 45 59 71 14
a3dcbdfa8ba91a736d6e04ee275e303ea486dea9f8410f28b347d52d902e3f2d  rows-across-shapes.pes  5 21 7 52 24
106ede243bd22cf2b6b5dfc06deb997c7a5d46fb63eb520aad7512bffe3d1920  rows-across-shapes.pec  5 21 7 52 24
91798558fcc2d19a8988a1d12caa736a9c56d0c36b3c1c202606392ba5ffb1bb  rows-across-shapes.jef  33 59 59 71 14
93940b8a9f473c88baee07695ebdcd3a4c01c189339e1b697807f0f3c703f3a2  rows-at-30.pes  5 21 7 52 24
2c9d4f741cb10278d9759445334b895fcb2131b81bc105ce2564650706b5b26f  rows-at-30.pec  5 21 7 52 24
a96f8c3765861e103e234212e1ad1fa173751e309b63734ae5fced8a5087bb1a  rows-at-30.jef  33 59 59 71 14
";

/// The PES, PEC and JEF exports of [`ARTWORK`] apart from their thread numbers, listed as
/// [`SYNTHETIC_THREADS`] is.
const ARTWORK_THREADS: &str = "
ad7ed95f42f840a6375ed04cae156899428b0960b864f33d938bff36ca51edd5  bullseye-twemoji.pes  29 32 5 33 20 36
0202de20bd2aed40d5c0afc42749fee4c8316c0c1faf21a1130c7990f6d91e48  bullseye-twemoji.pec  29 32 5 33 20 36
8e6f731f91aa084869c0d0539c428f1fe1204e639c9ec9d47af6fecb7d73cd24  bullseye-twemoji.jef  23 28 33 72 1 11
e35d5b6c303b6250fd4dfd44f1fb775c80427b30eca2e1993c4ef247d8938b84  fox-twemoji.clipart.pes  52 12 20 51
d2759f9a0ed526c111f0d1a52d0c333aead012b166cb5c4cd68ce3a38d454fb2  fox-twemoji.clipart.pec  52 12 20 51
a5f8b7f74f906f9fdc1dc327bf2c412899debe767f8184960754c05f6a432790  fox-twemoji.clipart.jef  71 13 1 42
f2a46cf6c82744875139fdbc944adf8ccd8b8182d2bb628777912880a53e9b77  heart-suit-twemoji.clipart.pes  51
3d3c6ad95ae331ffc87ef99be9a1970982ea49d6946d058b3f43dc420bb8ee8c  heart-suit-twemoji.clipart.pec  51
6aff83c9450dd6938908d2c861b0d6706f4b7e4b467ca63faaf79f6afebf4eb5  heart-suit-twemoji.clipart.jef  42
d089dc46da0237084444228270c664800d9e8699f79d415a98507e4528e4db54  joystick-twemoji.pes  16 48 23 39
82df3201c5c2065b6951da452ccae95c9f14194242913a74226480a09c8ef42b  joystick-twemoji.pec  16 48 23 39
fdefe110781dd8f72733500c2710bc386d7a8d1638cb38f5d9bab22c90cd13db  joystick-twemoji.jef  21 33 50 50
26635749f6186659b68389360c545d899c3806f5f42154634b2c83885c58721f  pool-8-ball-twemoji.pes  23 29 23
f3e2fcfac42c0392cd152e704c94ae33f077317470f8cc23d3b112780643c0be  pool-8-ball-twemoji.pec  23 29 23
b8ba6ec2c32a933ead4d62590462fabf0f5d584a6eed60375fc852e663ca6366  pool-8-ball-twemoji.jef  50 23 50
e7ce3d3eff96c25d0d6999499dee425c4f835f4aaf89a98ae7d4761258666fb5  puzzle-piece-twemoji.pes  37
5f11648c36420c97e8497a9648e9d0b4c20e93256fbd5541e4881e81a0a15ebd  puzzle-piece-twemoji.pec  37
e163888c72bbe1ffb2f5105fc1b7e6b8ead5ba84485186006dcf728cf3f75f94  puzzle-piece-twemoji.jef  22
77fed8ba45db2b9eadabe33e0c861c4b2de967aba8780690da423b80ff8bc001  smiling-face-with-smiling-eyes-twemoji.clipart.pes  58 43 23
b6f95525c4b1b399151870fa0848aec64644e1e7c8a2480cb43f2148ecdcbd85  smiling-face-with-smiling-eyes-twemoji.clipart.pec  58 43 23
54c4f38826cd3f30144d15233608ca0773122e12566b2e68a59468c4e8638aa1  smiling-face-with-smiling-eyes-twemoji.clipart.jef  73 9 1
45aa1daa47933e4900f57cbb9ba38ee0871db91b1ec1399c9474e3efea840969  sos-button-fluent.clipart.pes  48 29
dc50caa6ed08ece72977a1ad684a1fcc430c8083623f81caa8cc07de29391e5f  sos-button-fluent.clipart.pec  48 29
1655323c8826f956e0c0378e869c9047929a68daecdafcaf67d056d8ddb8bfaa  sos-button-fluent.clipart.jef  32 2
4fe219878c8f6e46cab1adc29f3bc4f1dd50c03d36773161852a10e70b666038  video-game-twemoji.pes  23 20 39 12 37 32 5 39
76d7cbe5d5b7f9593508af0a142e8be927b8196ca4b62b98884aaa0e805b867b  video-game-twemoji.pec  23 20 39 12 37 32 5 39
c756a88e1587a1408007f4154c09ed2e92d9c7ca6cfc60094e9d388090a0cbb5  video-game-twemoji.jef  50 1 50 37 22 28 33 24
24578cc59fe32fd1649431f9013b33db94a6f640838d0d642ea8ee7d4bb09c3d  watermelon-twemoji.clipart.pes  54 29 5 20
6934839e22399992315ac374ce7ae7192df9baa3aa13f941a53a9edc2e4dd5db  watermelon-twemoji.clipart.pec  54 29 5 20
6b9dea1b44e7959e9b1aaedadbb1c4d38a96bab45b3c051207bf486f7207e034  watermelon-twemoji.clipart.jef  6 51 42 1
f822791197122dc16f5f3be1d261142ec72b2410577d3766c5f6da538e50534c  zebra-twemoji.clipart.pes  29 20 29 39 23 20 39 20
85aa6e52df467d9da4e6f1ca82bae809f2e36bda7b9168b9fdfb58780fa6fa39  zebra-twemoji.clipart.pec  29 20 29 39 23 20 39 20
ec403fba27abaf795fa2d489cb0839970525d39394795f13fd6f5b0b01f2f474  zebra-twemoji.clipart.jef  51 1 51 50 1 1 50 1
";

/// What each export of [`SYNTHETIC`] holds, read back ([`Summary`]): its name, then its fields.
const SYNTHETIC_SUMMARIES: &str = "
fills.dst  blocks 4  needle 360/139/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.67  shortest 0.32  cells 575
fills.tap  blocks 4  needle 360/139/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.67  shortest 0.32  cells 575
fills.t01  blocks 4  needle 360/139/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.67  shortest 0.32  cells 575
fills.exp  blocks 4  needle 360/139/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.67  shortest 0.32  cells 575
fills.pes  blocks 4  needle 360/139/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.36  cells 573
fills.pec  blocks 4  needle 360/139/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.36  cells 573
fills.jef  blocks 4  needle 360/139/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.9  longest 4.60  shortest 0.36  cells 576
fills.vp3  blocks 4  needle 360/139/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.9  longest 4.60  shortest 0.36  cells 576
strokes.dst  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.50  cells 1592
strokes.tap  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.50  cells 1592
strokes.t01  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.50  cells 1592
strokes.exp  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.50  cells 1592
strokes.pes  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.42  cells 1593
strokes.pec  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.42  cells 1593
strokes.jef  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.2x89.5  longest 4.67  shortest 0.50  cells 1598
strokes.vp3  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.2x89.5  longest 4.67  shortest 0.50  cells 1598
compound-path-with-a-hole.dst  blocks 2  needle 1008/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.tap  blocks 2  needle 1008/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.t01  blocks 2  needle 1008/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.exp  blocks 2  needle 1008/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.pes  blocks 2  needle 1008/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.pec  blocks 2  needle 1008/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.jef  blocks 2  needle 1008/362  jumps 1/4  trims 0/3  size 74.3x28.5  longest 4.67  shortest 0.36  cells 1068
compound-path-with-a-hole.vp3  blocks 2  needle 1008/362  jumps 1/4  trims 0/3  size 74.3x28.5  longest 4.67  shortest 0.36  cells 1068
clip-groups.dst  blocks 5  needle 1178/1177/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.32  cells 1191
clip-groups.tap  blocks 5  needle 1178/1177/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.32  cells 1191
clip-groups.t01  blocks 5  needle 1178/1177/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.32  cells 1191
clip-groups.exp  blocks 5  needle 1178/1177/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.32  cells 1191
clip-groups.pes  blocks 5  needle 1178/1177/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1192
clip-groups.pec  blocks 5  needle 1178/1177/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1192
clip-groups.jef  blocks 5  needle 1178/1177/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1192
clip-groups.vp3  blocks 5  needle 1178/1177/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1192
symbols.dst  blocks 2  needle 698/437  jumps 3/3  trims 2/2  size 51.4x74.2  longest 4.56  shortest 0.41  cells 560
symbols.tap  blocks 2  needle 698/437  jumps 3/3  trims 2/2  size 51.4x74.2  longest 4.56  shortest 0.41  cells 560
symbols.t01  blocks 2  needle 698/437  jumps 3/3  trims 2/2  size 51.4x74.2  longest 4.56  shortest 0.41  cells 560
symbols.exp  blocks 2  needle 698/437  jumps 3/3  trims 2/2  size 51.4x74.2  longest 4.56  shortest 0.41  cells 560
symbols.pes  blocks 2  needle 698/437  jumps 3/3  trims 2/2  size 51.4x74.2  longest 4.56  shortest 0.42  cells 561
symbols.pec  blocks 2  needle 698/437  jumps 3/3  trims 2/2  size 51.4x74.2  longest 4.56  shortest 0.42  cells 561
symbols.jef  blocks 2  needle 698/437  jumps 3/3  trims 2/2  size 51.4x74.2  longest 4.56  shortest 0.42  cells 562
symbols.vp3  blocks 2  needle 698/437  jumps 3/3  trims 2/2  size 51.4x74.2  longest 4.56  shortest 0.42  cells 562
trim-after.dst  blocks 1  needle 245  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.36  cells 144
trim-after.tap  blocks 1  needle 245  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.36  cells 144
trim-after.t01  blocks 1  needle 245  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.36  cells 144
trim-after.exp  blocks 1  needle 245  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.36  cells 144
trim-after.pes  blocks 1  needle 245  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.36  cells 144
trim-after.pec  blocks 1  needle 245  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.36  cells 144
trim-after.jef  blocks 1  needle 245  jumps 4  trims 2  size 26.8x5.6  longest 4.53  shortest 0.36  cells 144
trim-after.vp3  blocks 1  needle 245  jumps 4  trims 2  size 26.8x5.6  longest 4.53  shortest 0.36  cells 144
opened-design.dst  blocks 2  needle 360/139  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.tap  blocks 2  needle 360/139  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.t01  blocks 2  needle 360/139  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.exp  blocks 2  needle 360/139  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.pes  blocks 2  needle 360/139  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.pec  blocks 2  needle 360/139  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.jef  blocks 2  needle 360/139  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.vp3  blocks 2  needle 360/139  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design-clipped.dst  blocks 1  needle 119  jumps 15  trims 1  size 7.3x10.9  longest 4.46  shortest 0.41  cells 80
opened-design-clipped.tap  blocks 1  needle 119  jumps 15  trims 1  size 7.3x10.9  longest 4.46  shortest 0.41  cells 80
opened-design-clipped.t01  blocks 1  needle 119  jumps 15  trims 1  size 7.3x10.9  longest 4.46  shortest 0.41  cells 80
opened-design-clipped.exp  blocks 1  needle 119  jumps 15  trims 1  size 7.3x10.9  longest 4.46  shortest 0.41  cells 80
opened-design-clipped.pes  blocks 1  needle 119  jumps 15  trims 1  size 7.3x10.9  longest 4.46  shortest 0.41  cells 80
opened-design-clipped.pec  blocks 1  needle 119  jumps 15  trims 1  size 7.3x10.9  longest 4.46  shortest 0.41  cells 80
opened-design-clipped.jef  blocks 1  needle 119  jumps 15  trims 1  size 7.3x10.9  longest 4.46  shortest 0.41  cells 80
opened-design-clipped.vp3  blocks 1  needle 119  jumps 15  trims 1  size 7.3x10.9  longest 4.46  shortest 0.41  cells 80
opened-design-cut-at-block-ends.dst  blocks 2  needle 20/20  jumps 2/2  trims 0/1  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.tap  blocks 2  needle 20/20  jumps 2/2  trims 0/1  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.t01  blocks 2  needle 20/20  jumps 2/2  trims 0/1  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.exp  blocks 2  needle 20/20  jumps 1/2  trims 1/2  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.pes  blocks 2  needle 20/20  jumps 2/2  trims 1/2  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.pec  blocks 2  needle 20/20  jumps 2/2  trims 1/2  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.jef  blocks 2  needle 20/20  jumps 2/2  trims 1/2  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
opened-design-cut-at-block-ends.vp3  blocks 2  needle 20/20  jumps 2/2  trims 0/1  size 58.0x10.0  longest 2.00  shortest 2.00  cells 76
mixed-stitch-style-fill.dst  blocks 6  needle 393/332/337/102/546/412  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1443
mixed-stitch-style-fill.tap  blocks 6  needle 393/332/337/102/546/412  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1443
mixed-stitch-style-fill.t01  blocks 6  needle 393/332/337/102/546/412  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1443
mixed-stitch-style-fill.exp  blocks 6  needle 393/332/337/102/546/412  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1443
mixed-stitch-style-fill.pes  blocks 6  needle 393/332/337/102/546/412  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1444
mixed-stitch-style-fill.pec  blocks 6  needle 393/332/337/102/546/412  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1444
mixed-stitch-style-fill.jef  blocks 6  needle 393/332/337/102/546/412  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.32  cells 1440
mixed-stitch-style-fill.vp3  blocks 6  needle 393/332/337/102/546/412  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.32  cells 1440
mixed-stitch-style-satin.dst  blocks 6  needle 393/300/369/365/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1486
mixed-stitch-style-satin.tap  blocks 6  needle 393/300/369/365/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1486
mixed-stitch-style-satin.t01  blocks 6  needle 393/300/369/365/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1486
mixed-stitch-style-satin.exp  blocks 6  needle 393/300/369/365/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1486
mixed-stitch-style-satin.pes  blocks 6  needle 393/300/369/365/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.60  shortest 0.40  cells 1485
mixed-stitch-style-satin.pec  blocks 6  needle 393/300/369/365/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.60  shortest 0.40  cells 1485
mixed-stitch-style-satin.jef  blocks 6  needle 393/300/369/365/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.67  shortest 0.36  cells 1489
mixed-stitch-style-satin.vp3  blocks 6  needle 393/300/369/365/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.67  shortest 0.36  cells 1489
mixed-stitch-style-running.dst  blocks 6  needle 35/48/31/37/41/41  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 437
mixed-stitch-style-running.tap  blocks 6  needle 35/48/31/37/41/41  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 437
mixed-stitch-style-running.t01  blocks 6  needle 35/48/31/37/41/41  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 437
mixed-stitch-style-running.exp  blocks 6  needle 35/48/31/37/41/41  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 437
mixed-stitch-style-running.pes  blocks 6  needle 35/48/31/37/41/41  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 440
mixed-stitch-style-running.pec  blocks 6  needle 35/48/31/37/41/41  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 84.7x77.6  longest 3.00  shortest 0.98  cells 440
mixed-stitch-style-running.jef  blocks 6  needle 35/48/31/37/41/41  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.6x77.6  longest 3.00  shortest 0.98  cells 436
mixed-stitch-style-running.vp3  blocks 6  needle 35/48/31/37/41/41  jumps 1/1/1/1/1/0  trims 0/0/0/0/0/0  size 84.6x77.6  longest 3.00  shortest 0.98  cells 436
mixed-every-option.dst  blocks 6  needle 297/232/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.45  cells 1722
mixed-every-option.tap  blocks 6  needle 297/232/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.45  cells 1722
mixed-every-option.t01  blocks 6  needle 297/232/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.45  cells 1722
mixed-every-option.exp  blocks 6  needle 297/232/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.45  cells 1722
mixed-every-option.pes  blocks 6  needle 297/232/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.45  cells 1723
mixed-every-option.pec  blocks 6  needle 297/232/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 88.9x80.4  longest 5.28  shortest 0.45  cells 1723
mixed-every-option.jef  blocks 6  needle 297/232/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 89.0x80.4  longest 5.33  shortest 0.50  cells 1735
mixed-every-option.vp3  blocks 6  needle 297/232/270/33/415/346  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 89.0x80.4  longest 5.33  shortest 0.50  cells 1735
mixed-shrunk.dst  blocks 6  needle 378/298/368/37/520/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.32  cells 1358
mixed-shrunk.tap  blocks 6  needle 378/298/368/37/520/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.32  cells 1358
mixed-shrunk.t01  blocks 6  needle 378/298/368/37/520/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.32  cells 1358
mixed-shrunk.exp  blocks 6  needle 378/298/368/37/520/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.32  cells 1358
mixed-shrunk.pes  blocks 6  needle 378/298/368/37/520/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.36  cells 1357
mixed-shrunk.pec  blocks 6  needle 378/298/368/37/520/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.36  cells 1357
mixed-shrunk.jef  blocks 6  needle 378/298/368/37/520/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.36  cells 1358
mixed-shrunk.vp3  blocks 6  needle 378/298/368/37/520/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.36  cells 1358
settings.dst  blocks 5  needle 518/370/431/438/467  jumps 1/1/1/1/1  trims 0/0/0/0/0  size 86.1x79.1  longest 3.81  shortest 0.32  cells 1295
settings.tap  blocks 5  needle 518/370/431/438/467  jumps 1/1/1/1/1  trims 0/0/0/0/0  size 86.1x79.1  longest 3.81  shortest 0.32  cells 1295
settings.t01  blocks 5  needle 518/370/431/438/467  jumps 1/1/1/1/1  trims 0/0/0/0/0  size 86.1x79.1  longest 3.81  shortest 0.32  cells 1295
settings.exp  blocks 5  needle 518/370/431/438/467  jumps 1/1/1/1/1  trims 0/0/0/0/0  size 86.1x79.1  longest 3.81  shortest 0.32  cells 1295
settings.pes  blocks 5  needle 518/370/431/438/467  jumps 1/1/1/1/1  trims 0/0/0/0/0  size 86.1x79.1  longest 3.81  shortest 0.30  cells 1326
settings.pec  blocks 5  needle 518/370/431/438/467  jumps 1/1/1/1/1  trims 0/0/0/0/0  size 86.1x79.1  longest 3.81  shortest 0.30  cells 1326
settings.jef  blocks 5  needle 518/370/431/438/467  jumps 1/1/1/1/1  trims 0/0/0/0/0  size 86.0x79.1  longest 3.81  shortest 0.32  cells 1302
settings.vp3  blocks 5  needle 518/370/431/438/467  jumps 1/1/1/1/1  trims 0/0/0/0/0  size 86.0x79.1  longest 3.81  shortest 0.32  cells 1302
expanded-strokes.dst  blocks 5  needle 693/593/703/1021/73  jumps 1/2/1/2/1  trims 0/0/0/0/0  size 73.2x65.6  longest 4.60  shortest 0.41  cells 1640
expanded-strokes.tap  blocks 5  needle 693/593/703/1021/73  jumps 1/2/1/2/1  trims 0/0/0/0/0  size 73.2x65.6  longest 4.60  shortest 0.41  cells 1640
expanded-strokes.t01  blocks 5  needle 693/593/703/1021/73  jumps 1/2/1/2/1  trims 0/0/0/0/0  size 73.2x65.6  longest 4.60  shortest 0.41  cells 1640
expanded-strokes.exp  blocks 5  needle 693/593/703/1021/73  jumps 1/2/1/2/1  trims 0/0/0/0/0  size 73.2x65.6  longest 4.60  shortest 0.41  cells 1640
expanded-strokes.pes  blocks 5  needle 693/593/703/1021/73  jumps 1/2/1/2/1  trims 0/0/0/0/0  size 73.2x65.6  longest 4.60  shortest 0.41  cells 1641
expanded-strokes.pec  blocks 5  needle 693/593/703/1021/73  jumps 1/2/1/2/1  trims 0/0/0/0/0  size 73.2x65.6  longest 4.60  shortest 0.41  cells 1641
expanded-strokes.jef  blocks 5  needle 693/593/703/1021/73  jumps 1/2/1/2/1  trims 0/0/0/0/0  size 73.2x65.6  longest 4.60  shortest 0.50  cells 1635
expanded-strokes.vp3  blocks 5  needle 693/593/703/1021/73  jumps 1/2/1/2/1  trims 0/0/0/0/0  size 73.2x65.6  longest 4.60  shortest 0.50  cells 1635
exits.dst  blocks 4  needle 613/25/480/49  jumps 1/1/1/1  trims 0/0/0/0  size 88.3x74.2  longest 6.40  shortest 0.41  cells 1138
exits.tap  blocks 4  needle 613/25/480/49  jumps 1/1/1/1  trims 0/0/0/0  size 88.3x74.2  longest 6.40  shortest 0.41  cells 1138
exits.t01  blocks 4  needle 613/25/480/49  jumps 1/1/1/1  trims 0/0/0/0  size 88.3x74.2  longest 6.40  shortest 0.41  cells 1138
exits.exp  blocks 4  needle 613/25/480/49  jumps 1/1/1/1  trims 0/0/0/0  size 88.3x74.2  longest 6.40  shortest 0.41  cells 1138
exits.pes  blocks 4  needle 613/25/480/49  jumps 1/1/1/1  trims 0/0/0/0  size 88.3x74.2  longest 6.40  shortest 0.41  cells 1139
exits.pec  blocks 4  needle 613/25/480/49  jumps 1/1/1/1  trims 0/0/0/0  size 88.3x74.2  longest 6.40  shortest 0.41  cells 1139
exits.jef  blocks 4  needle 613/25/480/49  jumps 1/1/1/1  trims 0/0/0/0  size 88.3x74.2  longest 6.41  shortest 0.41  cells 1137
exits.vp3  blocks 4  needle 613/25/480/49  jumps 1/1/1/1  trims 0/0/0/0  size 88.3x74.2  longest 6.41  shortest 0.41  cells 1137
stops.dst  blocks 9  needle 5/5/10/88/87/24/3/7/5  jumps 0/1/1/1/1/2/1/3/1  trims 0/0/0/0/0/0/0/0/0  size 55.1x55.0  longest 4.46  shortest 0.50  cells 250
stops.tap  blocks 9  needle 5/5/10/88/87/24/3/7/5  jumps 0/1/1/1/1/2/1/3/1  trims 0/0/0/0/0/0/0/0/0  size 55.1x55.0  longest 4.46  shortest 0.50  cells 250
stops.t01  blocks 9  needle 5/5/10/88/87/24/3/7/5  jumps 0/1/1/1/1/2/1/3/1  trims 0/0/0/0/0/0/0/0/0  size 55.1x55.0  longest 4.46  shortest 0.50  cells 250
stops.exp  blocks 9  needle 5/5/10/88/87/24/3/7/5  jumps 0/1/1/1/1/1/1/2/1  trims 0/0/0/0/1/1/0/1/0  size 55.1x55.0  longest 4.46  shortest 0.50  cells 250
stops.pes  blocks 9  needle 5/5/10/88/87/24/3/7/5  jumps 1/1/1/1/1/1/1/2/1  trims 0/0/0/0/1/1/0/1/0  size 55.1x55.0  longest 4.46  shortest 0.50  cells 250
stops.pec  blocks 9  needle 5/5/10/88/87/24/3/7/5  jumps 1/1/1/1/1/1/1/2/1  trims 0/0/0/0/1/1/0/1/0  size 55.1x55.0  longest 4.46  shortest 0.50  cells 250
stops.jef  blocks 9  needle 5/5/10/88/87/24/3/7/5  jumps 1/1/1/1/1/1/1/2/1  trims 0/0/0/0/1/1/0/1/0  size 55.1x55.0  longest 4.46  shortest 0.50  cells 250
stops.vp3  blocks 9  needle 5/5/10/88/87/24/3/7/5  jumps 1/1/1/1/1/1/1/2/1  trims 0/0/0/0/0/0/0/0/0  size 55.1x55.0  longest 4.46  shortest 0.50  cells 250
capped-strokes.dst  blocks 6  needle 346/379/334/390/441/403  jumps 1/2/1/1/1/1  trims 0/1/0/0/0/0  size 70.1x89.8  longest 4.02  shortest 0.80  cells 1677
capped-strokes.tap  blocks 6  needle 346/379/334/390/441/403  jumps 1/2/1/1/1/1  trims 0/1/0/0/0/0  size 70.1x89.8  longest 4.02  shortest 0.80  cells 1677
capped-strokes.t01  blocks 6  needle 346/379/334/390/441/403  jumps 1/2/1/1/1/1  trims 0/1/0/0/0/0  size 70.1x89.8  longest 4.02  shortest 0.80  cells 1677
capped-strokes.exp  blocks 6  needle 346/379/334/390/441/403  jumps 1/2/1/1/1/1  trims 0/1/0/0/0/0  size 70.1x89.8  longest 4.02  shortest 0.80  cells 1677
capped-strokes.pes  blocks 6  needle 346/379/334/390/441/403  jumps 1/2/1/1/1/1  trims 0/1/0/0/0/0  size 70.1x89.8  longest 4.02  shortest 0.80  cells 1678
capped-strokes.pec  blocks 6  needle 346/379/334/390/441/403  jumps 1/2/1/1/1/1  trims 0/1/0/0/0/0  size 70.1x89.8  longest 4.02  shortest 0.80  cells 1678
capped-strokes.jef  blocks 6  needle 346/379/334/390/441/403  jumps 1/2/1/1/1/1  trims 0/1/0/0/0/0  size 70.2x89.8  longest 4.03  shortest 0.80  cells 1671
capped-strokes.vp3  blocks 6  needle 346/379/334/390/441/403  jumps 1/2/1/1/1/1  trims 0/1/0/0/0/0  size 70.2x89.8  longest 4.03  shortest 0.80  cells 1671
rows-across-shapes.dst  blocks 5  needle 578/577/1190/426/516  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.4  longest 4.60  shortest 0.30  cells 2880
rows-across-shapes.tap  blocks 5  needle 578/577/1190/426/516  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.4  longest 4.60  shortest 0.30  cells 2880
rows-across-shapes.t01  blocks 5  needle 578/577/1190/426/516  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.4  longest 4.60  shortest 0.30  cells 2880
rows-across-shapes.exp  blocks 5  needle 578/577/1190/426/516  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.4  longest 4.60  shortest 0.30  cells 2880
rows-across-shapes.pes  blocks 5  needle 578/577/1190/426/516  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.4  longest 4.60  shortest 0.30  cells 2879
rows-across-shapes.pec  blocks 5  needle 578/577/1190/426/516  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.4  longest 4.60  shortest 0.30  cells 2879
rows-across-shapes.jef  blocks 5  needle 578/577/1190/426/516  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.4  longest 4.60  shortest 0.32  cells 2789
rows-across-shapes.vp3  blocks 5  needle 578/577/1190/426/516  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.4  longest 4.60  shortest 0.32  cells 2789
rows-at-30.dst  blocks 5  needle 603/558/1113/425/513  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.3  longest 4.61  shortest 0.36  cells 2793
rows-at-30.tap  blocks 5  needle 603/558/1113/425/513  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.3  longest 4.61  shortest 0.36  cells 2793
rows-at-30.t01  blocks 5  needle 603/558/1113/425/513  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.3  longest 4.61  shortest 0.36  cells 2793
rows-at-30.exp  blocks 5  needle 603/558/1113/425/513  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.3  longest 4.61  shortest 0.36  cells 2793
rows-at-30.pes  blocks 5  needle 603/558/1113/425/513  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.3  longest 4.61  shortest 0.36  cells 2796
rows-at-30.pec  blocks 5  needle 603/558/1113/425/513  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.3  longest 4.61  shortest 0.36  cells 2796
rows-at-30.jef  blocks 5  needle 603/558/1113/425/513  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.2  longest 4.61  shortest 0.32  cells 2798
rows-at-30.vp3  blocks 5  needle 603/558/1113/425/513  jumps 1/1/2/1/1  trims 0/0/1/0/0  size 80.8x102.2  longest 4.61  shortest 0.32  cells 2798
";

/// What each export of [`ARTWORK`] holds, read back, listed as [`SYNTHETIC_SUMMARIES`] is.
const ARTWORK_SUMMARIES: &str = "
bullseye-twemoji.dst  blocks 6  needle 1585/1015/566/245/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1713
bullseye-twemoji.tap  blocks 6  needle 1585/1015/566/245/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1713
bullseye-twemoji.t01  blocks 6  needle 1585/1015/566/245/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1713
bullseye-twemoji.exp  blocks 6  needle 1585/1015/566/245/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1713
bullseye-twemoji.pes  blocks 6  needle 1585/1015/566/245/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.50  shortest 0.32  cells 1715
bullseye-twemoji.pec  blocks 6  needle 1585/1015/566/245/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.50  shortest 0.32  cells 1715
bullseye-twemoji.jef  blocks 6  needle 1585/1015/566/245/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.2x45.2  longest 8.50  shortest 0.32  cells 1714
bullseye-twemoji.vp3  blocks 6  needle 1585/1015/566/245/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.2x45.2  longest 8.50  shortest 0.32  cells 1714
fox-twemoji.clipart.dst  blocks 4  needle 1442/464/152/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.04  shortest 0.32  cells 1331
fox-twemoji.clipart.tap  blocks 4  needle 1442/464/152/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.04  shortest 0.32  cells 1331
fox-twemoji.clipart.t01  blocks 4  needle 1442/464/152/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.04  shortest 0.32  cells 1331
fox-twemoji.clipart.exp  blocks 4  needle 1442/464/152/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.04  shortest 0.32  cells 1331
fox-twemoji.clipart.pes  blocks 4  needle 1442/464/152/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.06  shortest 0.32  cells 1331
fox-twemoji.clipart.pec  blocks 4  needle 1442/464/152/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.06  shortest 0.32  cells 1331
fox-twemoji.clipart.jef  blocks 4  needle 1442/464/152/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.32  cells 1335
fox-twemoji.clipart.vp3  blocks 4  needle 1442/464/152/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.32  cells 1335
heart-suit-twemoji.clipart.dst  blocks 1  needle 1406  jumps 1  trims 0  size 45.3x42.3  longest 4.53  shortest 0.32  cells 1370
heart-suit-twemoji.clipart.tap  blocks 1  needle 1406  jumps 1  trims 0  size 45.3x42.3  longest 4.53  shortest 0.32  cells 1370
heart-suit-twemoji.clipart.t01  blocks 1  needle 1406  jumps 1  trims 0  size 45.3x42.3  longest 4.53  shortest 0.32  cells 1370
heart-suit-twemoji.clipart.exp  blocks 1  needle 1406  jumps 1  trims 0  size 45.3x42.3  longest 4.53  shortest 0.32  cells 1370
heart-suit-twemoji.clipart.pes  blocks 1  needle 1406  jumps 1  trims 0  size 45.3x42.3  longest 4.53  shortest 0.32  cells 1372
heart-suit-twemoji.clipart.pec  blocks 1  needle 1406  jumps 1  trims 0  size 45.3x42.3  longest 4.53  shortest 0.32  cells 1372
heart-suit-twemoji.clipart.jef  blocks 1  needle 1406  jumps 1  trims 0  size 45.2x42.3  longest 4.53  shortest 0.32  cells 1372
heart-suit-twemoji.clipart.vp3  blocks 1  needle 1406  jumps 1  trims 0  size 45.2x42.3  longest 4.53  shortest 0.32  cells 1372
joystick-twemoji.dst  blocks 4  needle 155/260/1045/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.32  cells 1010
joystick-twemoji.tap  blocks 4  needle 155/260/1045/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.32  cells 1010
joystick-twemoji.t01  blocks 4  needle 155/260/1045/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.32  cells 1010
joystick-twemoji.exp  blocks 4  needle 155/260/1045/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.32  cells 1010
joystick-twemoji.pes  blocks 4  needle 155/260/1045/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.32  cells 1008
joystick-twemoji.pec  blocks 4  needle 155/260/1045/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.32  cells 1008
joystick-twemoji.jef  blocks 4  needle 155/260/1045/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.3  longest 5.70  shortest 0.32  cells 988
joystick-twemoji.vp3  blocks 4  needle 155/260/1045/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.3  longest 5.70  shortest 0.32  cells 988
pool-8-ball-twemoji.dst  blocks 3  needle 1667/492/320  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.67  shortest 0.32  cells 1759
pool-8-ball-twemoji.tap  blocks 3  needle 1667/492/320  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.67  shortest 0.32  cells 1759
pool-8-ball-twemoji.t01  blocks 3  needle 1667/492/320  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.67  shortest 0.32  cells 1759
pool-8-ball-twemoji.exp  blocks 3  needle 1667/492/320  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.67  shortest 0.32  cells 1759
pool-8-ball-twemoji.pes  blocks 3  needle 1667/492/320  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.60  shortest 0.32  cells 1759
pool-8-ball-twemoji.pec  blocks 3  needle 1667/492/320  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.60  shortest 0.32  cells 1759
pool-8-ball-twemoji.jef  blocks 3  needle 1667/492/320  jumps 1/1/1  trims 0/0/0  size 46.2x46.2  longest 4.60  shortest 0.32  cells 1757
pool-8-ball-twemoji.vp3  blocks 3  needle 1667/492/320  jumps 1/1/1  trims 0/0/0  size 46.2x46.2  longest 4.60  shortest 0.32  cells 1757
puzzle-piece-twemoji.dst  blocks 1  needle 1393  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.32  cells 1205
puzzle-piece-twemoji.tap  blocks 1  needle 1393  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.32  cells 1205
puzzle-piece-twemoji.t01  blocks 1  needle 1393  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.32  cells 1205
puzzle-piece-twemoji.exp  blocks 1  needle 1393  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.32  cells 1205
puzzle-piece-twemoji.pes  blocks 1  needle 1393  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.32  cells 1204
puzzle-piece-twemoji.pec  blocks 1  needle 1393  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.32  cells 1204
puzzle-piece-twemoji.jef  blocks 1  needle 1393  jumps 1  trims 0  size 45.2x45.2  longest 4.60  shortest 0.32  cells 1206
puzzle-piece-twemoji.vp3  blocks 1  needle 1393  jumps 1  trims 0  size 45.2x45.2  longest 4.60  shortest 0.32  cells 1206
smiling-face-with-smiling-eyes-twemoji.clipart.dst  blocks 3  needle 1939/389/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1855
smiling-face-with-smiling-eyes-twemoji.clipart.tap  blocks 3  needle 1939/389/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1855
smiling-face-with-smiling-eyes-twemoji.clipart.t01  blocks 3  needle 1939/389/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1855
smiling-face-with-smiling-eyes-twemoji.clipart.exp  blocks 3  needle 1939/389/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1855
smiling-face-with-smiling-eyes-twemoji.clipart.pes  blocks 3  needle 1939/389/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1854
smiling-face-with-smiling-eyes-twemoji.clipart.pec  blocks 3  needle 1939/389/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1854
smiling-face-with-smiling-eyes-twemoji.clipart.jef  blocks 3  needle 1939/389/388  jumps 1/2/3  trims 0/1/0  size 47.7x47.7  longest 5.92  shortest 0.32  cells 1855
smiling-face-with-smiling-eyes-twemoji.clipart.vp3  blocks 3  needle 1939/389/388  jumps 1/2/3  trims 0/1/0  size 47.7x47.7  longest 5.92  shortest 0.32  cells 1855
sos-button-fluent.clipart.dst  blocks 2  needle 1994/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.67  shortest 0.36  cells 2076
sos-button-fluent.clipart.tap  blocks 2  needle 1994/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.67  shortest 0.36  cells 2076
sos-button-fluent.clipart.t01  blocks 2  needle 1994/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.67  shortest 0.36  cells 2076
sos-button-fluent.clipart.exp  blocks 2  needle 1994/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.67  shortest 0.36  cells 2076
sos-button-fluent.clipart.pes  blocks 2  needle 1994/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.36  cells 2076
sos-button-fluent.clipart.pec  blocks 2  needle 1994/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.36  cells 2076
sos-button-fluent.clipart.jef  blocks 2  needle 1994/770  jumps 1/3  trims 0/1  size 45.2x45.2  longest 4.60  shortest 0.32  cells 2075
sos-button-fluent.clipart.vp3  blocks 2  needle 1994/770  jumps 1/3  trims 0/1  size 45.2x45.2  longest 4.60  shortest 0.32  cells 2075
video-game-twemoji.dst  blocks 8  needle 612/1226/147/36/35/34/36/150  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.95  shortest 0.32  cells 1222
video-game-twemoji.tap  blocks 8  needle 612/1226/147/36/35/34/36/150  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.95  shortest 0.32  cells 1222
video-game-twemoji.t01  blocks 8  needle 612/1226/147/36/35/34/36/150  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.95  shortest 0.32  cells 1222
video-game-twemoji.exp  blocks 8  needle 612/1226/147/36/35/34/36/150  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.95  shortest 0.32  cells 1222
video-game-twemoji.pes  blocks 8  needle 612/1226/147/36/35/34/36/150  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.88  shortest 0.32  cells 1221
video-game-twemoji.pec  blocks 8  needle 612/1226/147/36/35/34/36/150  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.88  shortest 0.32  cells 1221
video-game-twemoji.jef  blocks 8  needle 612/1226/147/36/35/34/36/150  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.88  shortest 0.32  cells 1217
video-game-twemoji.vp3  blocks 8  needle 612/1226/147/36/35/34/36/150  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.88  shortest 0.32  cells 1217
watermelon-twemoji.clipart.dst  blocks 4  needle 411/367/612/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.32  cells 966
watermelon-twemoji.clipart.tap  blocks 4  needle 411/367/612/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.32  cells 966
watermelon-twemoji.clipart.t01  blocks 4  needle 411/367/612/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.32  cells 966
watermelon-twemoji.clipart.exp  blocks 4  needle 411/367/612/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.32  cells 966
watermelon-twemoji.clipart.pes  blocks 4  needle 411/367/612/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.32  cells 966
watermelon-twemoji.clipart.pec  blocks 4  needle 411/367/612/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.32  cells 966
watermelon-twemoji.clipart.jef  blocks 4  needle 411/367/612/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.74  shortest 0.32  cells 966
watermelon-twemoji.clipart.vp3  blocks 4  needle 411/367/612/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.74  shortest 0.32  cells 966
zebra-twemoji.clipart.dst  blocks 8  needle 1625/322/270/42/27/572/46/769  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1571
zebra-twemoji.clipart.tap  blocks 8  needle 1625/322/270/42/27/572/46/769  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1571
zebra-twemoji.clipart.t01  blocks 8  needle 1625/322/270/42/27/572/46/769  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1571
zebra-twemoji.clipart.exp  blocks 8  needle 1625/322/270/42/27/572/46/769  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1571
zebra-twemoji.clipart.pes  blocks 8  needle 1625/322/270/42/27/572/46/769  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
zebra-twemoji.clipart.pec  blocks 8  needle 1625/322/270/42/27/572/46/769  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
zebra-twemoji.clipart.jef  blocks 8  needle 1625/322/270/42/27/572/46/769  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1571
zebra-twemoji.clipart.vp3  blocks 8  needle 1625/322/270/42/27/572/46/769  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1571
";

/// The test sheet sewn on each fabric preset ([`the_test_sheet_on_each_fabric_exports_unchanged`]),
/// as [`SYNTHETIC`] lists the documents built here.
const FABRIC_SHEETS: &str = "
a991f50b7314b2631ee13bebb4f89b261e3eddb843546f740ea5b833be85f57e  sheet-cotton.dst
f74c5629d3cca24494764158f9935d0cc44fa895477e03e2c75b94796fd880ec  sheet-cotton.tap
f74c5629d3cca24494764158f9935d0cc44fa895477e03e2c75b94796fd880ec  sheet-cotton.t01
833ce5ab7414076da3263c5fb739d656efea1e4df1febe1b7f68aeb952822c78  sheet-cotton.exp
af073a1a68badb85ab23494eeb4b7acbfc6dc71270e5e18a1119c58a049023c6  sheet-cotton.pes
27a3f46f0a3636fb4d70e1ba584ae9a999bde621e535e315fdcc8f192e1fb790  sheet-cotton.pec
403216eb1c38a0ca5943710710171a16368a5ce53dcb3d902e3004985fb3ec1a  sheet-cotton.jef
5378f6e1032e8cca52754d2d0c2b5813a0727c8f26f0940c65553e847075de44  sheet-cotton.vp3
441eef9c83d6601b7e889d82d7285046221da22a76940bf5bb76b1a64fddb429  sheet-denim.dst
d5e3aa5a253271e4ae962fe23214ed1b0024584640f775a4e2027135851e9936  sheet-denim.tap
d5e3aa5a253271e4ae962fe23214ed1b0024584640f775a4e2027135851e9936  sheet-denim.t01
2c7158917d0ddc7ac78b2cfc0e36dde943cd296124ac3a2dfafce18d1636a53b  sheet-denim.exp
1039d9d8c75a38fcb4f8eb23ab1944121cafb8325044092915171cbe90624a38  sheet-denim.pes
c2fba2a8ba71129e59edf9b6d24933ff9f5d0020b08c16d08c097dc0a1a84259  sheet-denim.pec
a8945d8f134294bc361f0a82ff431b45c4a47d9a5f0736a1002ef78c3a088bd1  sheet-denim.jef
2c79437162e80abc4ed14b9dcb6f1965b262fd53aa1bfb3c9b76257df867c45f  sheet-denim.vp3
673ebb544f74802bf555aaf4bcd66199b13f762ebbf8da2c01ef95c3c60a34ef  sheet-stretchKnit.dst
2b92922cd3040ef312891c4ca5b887d72575e3c6ecbe45924273c048e57214a8  sheet-stretchKnit.tap
2b92922cd3040ef312891c4ca5b887d72575e3c6ecbe45924273c048e57214a8  sheet-stretchKnit.t01
8910f82fec9dc33a7b43a41c6128c77c82753850be542d4d4127cbf58f696ab7  sheet-stretchKnit.exp
7816e6b124f130e77d68d874cda3c069fc2a550d728fedfe6c33c2bd0f8ef1b9  sheet-stretchKnit.pes
e821e315aaf19639a34908829b8f225fef89466e746417687017200fd7d445ac  sheet-stretchKnit.pec
e3a9043758e46376306cb097a7972be4d255033d7c386e0ff804e1522f4a2e8e  sheet-stretchKnit.jef
64c70ce91572bd3de67ad2a5946c09c64f084a954f51fea699ff3398ce58a8aa  sheet-stretchKnit.vp3
d7c4dfe108047f2da70a4dcfa090d8c732ba546d4ce707a6473c2151fe58e310  sheet-pique.dst
e231766ced5b2cafb22dceb6331b6502b986b91ae1e8b7e147c788be46506a8a  sheet-pique.tap
e231766ced5b2cafb22dceb6331b6502b986b91ae1e8b7e147c788be46506a8a  sheet-pique.t01
216388f01115b658d356ffadef4df0b7253693177ba57880c1c9260d662e5b82  sheet-pique.exp
3875297c323ac8d0dffad5b9e9aeaf9923a6c64a0a92e74062aadd91ca6d977f  sheet-pique.pes
92696a67fba84a44c4f134036c577f21aab3f5b49ff547893385a85958b14810  sheet-pique.pec
0c4bf99a0e1ee83b0b66746fe37944fc4645f9fcc009f17be78bea8b3a0fd6a5  sheet-pique.jef
abb183de20bb07592978212a2426f911454398c5fe1668e6803a56afe0647713  sheet-pique.vp3
3b15959aa09a8c42138baa2246e97bb6c6a63129c457288557f1bc76812645d3  sheet-fleece.dst
ebe3f9bb82dbaa3a9e474b957964f0bb67a39ae52607b44b368b4f82062c5fda  sheet-fleece.tap
ebe3f9bb82dbaa3a9e474b957964f0bb67a39ae52607b44b368b4f82062c5fda  sheet-fleece.t01
11425af5e8f3889dd73d06b638724098d6b9cd24b26dddb5664d861e88d5ce05  sheet-fleece.exp
2aeb8b6c857c2b65fe2d49b12375d38f1c1e53488ea99efe1f3ea5543ea19b3e  sheet-fleece.pes
31c12148c1d8392617404433d8083f5ecdeea9984ec4ed4c0b6c68e982a6b912  sheet-fleece.pec
5d56b1ba9b1b14d12b29898d77ca2a88db87008589b11d6f637fee4c39ee41f1  sheet-fleece.jef
69847f54ea76d09d331445f15199c29345c11774241868f0b8403286a5aa034d  sheet-fleece.vp3
f55c329fc9b7f24769c39b2be46b0970aabaf45f2996c662b08485518f492eb3  sheet-silk.dst
60a77025e335dc85808188b0dd1cb30af12323c7a8e84b928688530fc5eb7839  sheet-silk.tap
60a77025e335dc85808188b0dd1cb30af12323c7a8e84b928688530fc5eb7839  sheet-silk.t01
fec0b38fdbdc6e90ba76e62f79708d4ec50ccf5649f319a27886e8fe5203a517  sheet-silk.exp
026d65cf957dc4d7bf1d1e2e33bc03aa28964854fb1f22c77bcab61f1c838c3c  sheet-silk.pes
79fd485a00f4560589261d8c084c2b5dd10ddc102ff159a4a164002fb3b3df6c  sheet-silk.pec
e85a84dc23989fe2e2269a22d4faa1c3a299324abfd7980a6928cb4933c8e1ee  sheet-silk.jef
7d7ab0a07bbfe15b4fb09c2b2adccbc2a95aa80e703684ebe0e05021f71a5f1f  sheet-silk.vp3
83d956c2fd1e0fa9cd864b8ea21aa723d58c8ab5ca146cf2f64fc5ca4d69db08  sheet-nylon.dst
f74c5629d3cca24494764158f9935d0cc44fa895477e03e2c75b94796fd880ec  sheet-nylon.tap
f74c5629d3cca24494764158f9935d0cc44fa895477e03e2c75b94796fd880ec  sheet-nylon.t01
833ce5ab7414076da3263c5fb739d656efea1e4df1febe1b7f68aeb952822c78  sheet-nylon.exp
87dba21f2812009fb4a3b836f9645a37f90dd31c7343a140b8073bff9a4afed9  sheet-nylon.pes
cd5c9a88fdc1f66b1fbd9f17c63143cbda511e9d1d7d4e4d236e18db4a002c85  sheet-nylon.pec
403216eb1c38a0ca5943710710171a16368a5ce53dcb3d902e3004985fb3ec1a  sheet-nylon.jef
5378f6e1032e8cca52754d2d0c2b5813a0727c8f26f0940c65553e847075de44  sheet-nylon.vp3
e9bf8909430934d066fdba487b99a3c38ad45c1cefdf7341749559e5416c3062  sheet-cap.dst
f74c5629d3cca24494764158f9935d0cc44fa895477e03e2c75b94796fd880ec  sheet-cap.tap
f74c5629d3cca24494764158f9935d0cc44fa895477e03e2c75b94796fd880ec  sheet-cap.t01
833ce5ab7414076da3263c5fb739d656efea1e4df1febe1b7f68aeb952822c78  sheet-cap.exp
0424de880240b02dc04158c8cdc9164283b8b1eb1a1fe75e425cc25910c4ecba  sheet-cap.pes
2123e340f2c555f2c66b53a47ee528c0b5dbdf08d4346b7fc851a161ad7f9095  sheet-cap.pec
403216eb1c38a0ca5943710710171a16368a5ce53dcb3d902e3004985fb3ec1a  sheet-cap.jef
5378f6e1032e8cca52754d2d0c2b5813a0727c8f26f0940c65553e847075de44  sheet-cap.vp3
";

/// Its PES, PEC and JEF exports apart from their thread numbers, as [`SYNTHETIC_THREADS`].
const FABRIC_SHEETS_THREADS: &str = "
bed7d27ea80d0efd53731a624210cb72fb9f7cd1109108e1d0f81d25239be555  sheet-cotton.pes  5 37 21 7 52 24
d8e4d5a8613859df8c591368e1619e2f421254f11f51acca6fcffc5d31847d37  sheet-cotton.pec  5 37 21 7 52 24
50dca9460abded6947ba795d812c40192a242d3c302a075a7e6832a6362a6b54  sheet-cotton.jef  33 45 59 59 71 14
4500f7e60144985fc3f58562f3577f319b51171b774ab114491267b7133ead82  sheet-denim.pes  5 37 21 7 52 24
d8f58ecbdb7b5c2eca4832bab395197768d7826f75c48a40cbd6e3d282d7e676  sheet-denim.pec  5 37 21 7 52 24
c4907ff9a77bd7c9f0eab96ae309582cf8dd62f8dac2dffd36c5cca74f02b353  sheet-denim.jef  33 45 59 59 71 14
3342333c76994a8203745947360920a8cee471e2b29b6a6611e3a69c0b85f763  sheet-stretchKnit.pes  5 37 21 7 52 24
d1e7b563e2d378d394227189a74ebed7a53d13ae7c31a6fd35893ffbca18b2ad  sheet-stretchKnit.pec  5 37 21 7 52 24
a5b21b214b37257dcb71be4c8e3de955dcfc9b3acea7c1f0728c0e64dac7ecfe  sheet-stretchKnit.jef  33 45 59 59 71 14
22c1694414ccf28bf0c18c232e5a4c0e74e198153e9cb2b2ca47416442dea9ee  sheet-pique.pes  5 37 21 7 52 24
0f266630d588abaa7117f2cbd31265bdcc31015a523c17ad34226ff7103a22e4  sheet-pique.pec  5 37 21 7 52 24
6b3547e20f613be36296d2eddf5dccb049e9fa2dccd02f8afa9ab1b9d9cfd8b4  sheet-pique.jef  33 45 59 59 71 14
0c47daed0dbe05cd527c71d03fe17bdda490856f1f37ceb88aafd018ce0b6c5b  sheet-fleece.pes  5 37 21 7 52 24
78881f2ba6c828ef507feb77ac0990db252a9cef050efc25667bf8eeb2c208c9  sheet-fleece.pec  5 37 21 7 52 24
5a0f953674937e843be89d657bdb86b5ba25e46f261458604cce73897ba6cabe  sheet-fleece.jef  33 45 59 59 71 14
9557ef149c229e4107dca2fcd486207436ed0db556776d00ee193a8be0d1e6fe  sheet-silk.pes  5 37 21 7 52 24
a66b7cb637bab83225c2f64f16ae3008a1a04f4193f15ed9b991eef710a7282f  sheet-silk.pec  5 37 21 7 52 24
5266e97b6e43321dad48e17b12e912944fa1eb8fe303aef35f1a1a331deca68d  sheet-silk.jef  33 45 59 59 71 14
a95e1df837efef6acc69edc6477e274f3cb522eb7a2dcdf64161b5f24838784a  sheet-nylon.pes  5 37 21 7 52 24
adddabb24d44530e158d951b5ec85e95c51fbef8e1f0f3836c04aa22bc9998a7  sheet-nylon.pec  5 37 21 7 52 24
50dca9460abded6947ba795d812c40192a242d3c302a075a7e6832a6362a6b54  sheet-nylon.jef  33 45 59 59 71 14
ce6a3d82a3db520cafc8407b20774489ca4a07969d3e029e4280a45e08c54ed5  sheet-cap.pes  5 37 21 7 52 24
8674acaad2746c990288f64d3aa4d1b150131510d52ec3bc83140adff2e3f318  sheet-cap.pec  5 37 21 7 52 24
50dca9460abded6947ba795d812c40192a242d3c302a075a7e6832a6362a6b54  sheet-cap.jef  33 45 59 59 71 14
";

/// What each holds, as [`SYNTHETIC_SUMMARIES`].
const FABRIC_SHEETS_SUMMARIES: &str = "
sheet-cotton.dst  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-cotton.tap  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-cotton.t01  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-cotton.exp  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-cotton.pes  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.60  shortest 0.40  cells 1436
sheet-cotton.pec  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.60  shortest 0.40  cells 1436
sheet-cotton.jef  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.67  shortest 0.36  cells 1433
sheet-cotton.vp3  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.67  shortest 0.36  cells 1433
sheet-denim.dst  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.41  cells 1440
sheet-denim.tap  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.41  cells 1440
sheet-denim.t01  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.41  cells 1440
sheet-denim.exp  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.41  cells 1440
sheet-denim.pes  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1436
sheet-denim.pec  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1436
sheet-denim.jef  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.60  shortest 0.36  cells 1433
sheet-denim.vp3  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.60  shortest 0.36  cells 1433
sheet-stretchKnit.dst  blocks 6  needle 393/300/370/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.0  longest 4.60  shortest 0.40  cells 1521
sheet-stretchKnit.tap  blocks 6  needle 393/300/370/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.0  longest 4.60  shortest 0.40  cells 1521
sheet-stretchKnit.t01  blocks 6  needle 393/300/370/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.0  longest 4.60  shortest 0.40  cells 1521
sheet-stretchKnit.exp  blocks 6  needle 393/300/370/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.0  longest 4.60  shortest 0.40  cells 1521
sheet-stretchKnit.pes  blocks 6  needle 393/300/370/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.0  longest 4.60  shortest 0.41  cells 1521
sheet-stretchKnit.pec  blocks 6  needle 393/300/370/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.0  longest 4.60  shortest 0.41  cells 1521
sheet-stretchKnit.jef  blocks 6  needle 393/300/370/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.0  longest 4.67  shortest 0.50  cells 1525
sheet-stretchKnit.vp3  blocks 6  needle 393/300/370/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.0  longest 4.67  shortest 0.50  cells 1525
sheet-pique.dst  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.8  longest 4.60  shortest 0.45  cells 1466
sheet-pique.tap  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.8  longest 4.60  shortest 0.45  cells 1466
sheet-pique.t01  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.8  longest 4.60  shortest 0.45  cells 1466
sheet-pique.exp  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.8  longest 4.60  shortest 0.45  cells 1466
sheet-pique.pes  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.8  longest 4.60  shortest 0.45  cells 1464
sheet-pique.pec  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.8  longest 4.60  shortest 0.45  cells 1464
sheet-pique.jef  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.2x78.8  longest 4.67  shortest 0.45  cells 1468
sheet-pique.vp3  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.2x78.8  longest 4.67  shortest 0.45  cells 1468
sheet-fleece.dst  blocks 6  needle 449/304/370/37/622/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.1  longest 4.46  shortest 0.40  cells 1531
sheet-fleece.tap  blocks 6  needle 449/304/370/37/622/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.1  longest 4.46  shortest 0.40  cells 1531
sheet-fleece.t01  blocks 6  needle 449/304/370/37/622/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.1  longest 4.46  shortest 0.40  cells 1531
sheet-fleece.exp  blocks 6  needle 449/304/370/37/622/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.1  longest 4.46  shortest 0.40  cells 1531
sheet-fleece.pes  blocks 6  needle 449/304/370/37/622/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.1  longest 4.46  shortest 0.40  cells 1537
sheet-fleece.pec  blocks 6  needle 449/304/370/37/622/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.1  longest 4.46  shortest 0.40  cells 1537
sheet-fleece.jef  blocks 6  needle 449/304/370/37/622/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.1  longest 4.46  shortest 0.40  cells 1532
sheet-fleece.vp3  blocks 6  needle 449/304/370/37/622/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x79.1  longest 4.46  shortest 0.40  cells 1532
sheet-silk.dst  blocks 6  needle 326/276/340/37/455/355  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.81  shortest 0.50  cells 1431
sheet-silk.tap  blocks 6  needle 326/276/340/37/455/355  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.81  shortest 0.50  cells 1431
sheet-silk.t01  blocks 6  needle 326/276/340/37/455/355  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.81  shortest 0.50  cells 1431
sheet-silk.exp  blocks 6  needle 326/276/340/37/455/355  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.81  shortest 0.50  cells 1431
sheet-silk.pes  blocks 6  needle 326/276/340/37/455/355  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.81  shortest 0.50  cells 1438
sheet-silk.pec  blocks 6  needle 326/276/340/37/455/355  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.81  shortest 0.50  cells 1438
sheet-silk.jef  blocks 6  needle 326/276/340/37/455/355  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.81  shortest 0.45  cells 1429
sheet-silk.vp3  blocks 6  needle 326/276/340/37/455/355  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.81  shortest 0.45  cells 1429
sheet-nylon.dst  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-nylon.tap  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-nylon.t01  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-nylon.exp  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-nylon.pes  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.60  shortest 0.40  cells 1436
sheet-nylon.pec  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.60  shortest 0.40  cells 1436
sheet-nylon.jef  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.67  shortest 0.36  cells 1433
sheet-nylon.vp3  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.67  shortest 0.36  cells 1433
sheet-cap.dst  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-cap.tap  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-cap.t01  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-cap.exp  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.53  shortest 0.40  cells 1440
sheet-cap.pes  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.60  shortest 0.40  cells 1436
sheet-cap.pec  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.1x78.7  longest 4.60  shortest 0.40  cells 1436
sheet-cap.jef  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.67  shortest 0.36  cells 1433
sheet-cap.vp3  blocks 6  needle 393/300/369/37/546/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 86.0x78.7  longest 4.67  shortest 0.36  cells 1433
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

/// The test sheet (fills, a letter whose stem is sewn as satin, a satin line, a running zigzag, a
/// filled and stroked square: [`mixed`]) sewn on each fabric preset, chosen with
/// `embroidery.setup {fabric}`, writes in every format what it wrote when the listing was recorded;
/// on the default fabric it writes, byte for byte, what it writes with no fabric.
#[test]
fn the_test_sheet_on_each_fabric_exports_unchanged() {
    let mut now = Vec::new();
    for f in vectorcraft_stitch::fabric::FABRICS {
        let mut s = mixed();
        run(&mut s, "embroidery.setup", json!({"fabric": f.id}));
        let name = format!("sheet-{}", f.id);
        run(&mut s, "file.info", json!({"title": name}));
        let plan = planned(&mut s, &json!({}));
        for format in Format::ALL {
            let bytes = export(&mut s, format, &json!({})).unwrap_or_else(|e| panic!("{name} as {}: {e}", format.id()));
            let pinned = Pinned::new(format!("{name}.{}", format.extension()), format, &bytes);
            plan.holds(&pinned, format);
            now.push(pinned);
        }
    }
    let default = format!("sheet-{}", vectorcraft_stitch::fabric::DEFAULT_FABRIC);
    let mut bare = mixed();
    run(&mut bare, "file.info", json!({"title": default}));
    for format in Format::ALL {
        let bytes = export(&mut bare, format, &json!({})).unwrap();
        let name = format!("{default}.{}", format.extension());
        let on_default = now.iter().find(|p| p.name == name).unwrap();
        assert_eq!(sha256::hex(&bytes), on_default.hash, "{name}: the default fabric sews as no fabric");
    }
    check(FABRIC_SHEETS, FABRIC_SHEETS_THREADS, FABRIC_SHEETS_SUMMARIES, &now);
}

/// What a session's design plans (`embroidery.stats`): each block's needle points and stops, and
/// its longest needle-down stitch (millimetres).
struct Planned {
    needle: Vec<usize>,
    stops: Vec<usize>,
    longest: f64,
}

/// The plan of the session's design exported with `options`.
fn planned(s: &mut Session, options: &Value) -> Planned {
    let mut options = options.clone();
    // Every block listed.
    options["limit"] = json!(10_000);
    let stats = run(s, "embroidery.stats", options);
    assert_eq!(stats["truncated"], json!(false));
    let blocks = stats["blocks"].as_array().unwrap();
    let needle = blocks.iter().map(|b| b["stitches"].as_u64().unwrap() as usize).collect();
    let stops = blocks.iter().map(|b| b["stops"].as_u64().unwrap() as usize).collect();
    Planned { needle, stops, longest: stats["longestStitch"].as_f64().unwrap_or(0.0) }
}

impl Planned {
    /// An export read back has the plan's needle points, block by block, when no planned stitch is
    /// longer than one of its format's records holds (a longer one is written as several).
    fn holds(&self, pinned: &Pinned, format: Format) {
        if self.longest <= format.max_step() {
            // A stop is written as a change to the same thread: a block with stops reads back as one
            // block more for each.
            let mut read = pinned.summary.needle.iter();
            let grouped: Vec<usize> = self.stops.iter().map(|&stops| read.by_ref().take(stops + 1).sum()).collect();
            assert_eq!(grouped, self.needle, "{}: the needle points read back are not the plan's", pinned.name);
            assert_eq!(read.next(), None, "{}: blocks read back that the plan doesn't have", pinned.name);
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

/// Embroidery settings that set nothing a plan reads change no export, of the documents built here
/// or of the real designs' artwork ([`Unchanging`]): what a newer version wrote at every level,
/// every value of every part set and then reset, and the default fabric copied into the document
/// with every machine value at its default and the built-in rows as the document's.
#[test]
fn settings_that_change_nothing_change_no_export() {
    let mut summaries: Vec<(String, String)> = Vec::new();
    for how in [Unchanging::Kept, Unchanging::SetAndReset, Unchanging::DefaultFabric, Unchanging::Converted, Unchanging::ConvertedAndRestored] {
        let mut now = Vec::new();
        for (name, s, options) in documents() {
            // The documents that set settings (or points) on purpose are exported as they are, but
            // converted to manual stitches as any other.
            let set_on_purpose = matches!(name, "settings" | "expanded-strokes" | "exits" | "rows-across-shapes" | "rows-at-30");
            let mut s = match how {
                Unchanging::Converted => {
                    let (s, summary) = converted(s, &options);
                    summaries.push((name.to_string(), summary));
                    s
                }
                Unchanging::ConvertedAndRestored => restored(converted(s, &options).0),
                _ if set_on_purpose => s,
                _ => unchanging(s, how),
            };
            run(&mut s, "file.info", json!({"title": name}));
            for format in Format::ALL {
                let bytes = export(&mut s, format, &options).unwrap_or_else(|e| panic!("{name} ({how:?}) as {}: {e}", format.id()));
                now.push(Pinned::new(format!("{name}.{}", format.extension()), format, &bytes));
            }
        }
        check(SYNTHETIC, SYNTHETIC_THREADS, SYNTHETIC_SUMMARIES, &now);
        let Some(dir) = corpus() else { continue };
        let mut now = Vec::new();
        for &(art, theirs) in DESIGNS {
            let stem = art.trim_end_matches(".svg");
            let mut s = match how {
                Unchanging::Converted => {
                    let (s, summary) = converted(artwork(&dir, art, theirs), &json!({}));
                    summaries.push((stem.to_string(), summary));
                    s
                }
                Unchanging::ConvertedAndRestored => restored(converted(artwork(&dir, art, theirs), &json!({})).0),
                _ => unchanging(artwork(&dir, art, theirs), how),
            };
            run(&mut s, "file.info", json!({"title": stem}));
            for format in Format::ALL {
                let bytes = export(&mut s, format, &json!({})).unwrap_or_else(|e| panic!("{art} ({how:?}) as {}: {e}", format.id()));
                now.push(Pinned::new(format!("{stem}.{}", format.extension()), format, &bytes));
            }
        }
        check(ARTWORK, ARTWORK_THREADS, ARTWORK_SUMMARIES, &now);
    }
    check_converted(&summaries);
}

/// Settings that set nothing a plan reads ([`settings_that_change_nothing_change_no_export`]).
#[derive(Clone, Copy, Debug)]
enum Unchanging {
    /// What a newer version wrote that this one doesn't read, at every level: on every object, a
    /// fill part and a stroke part holding only keys this version doesn't know (and a value of
    /// another type), and a key of its own; in the document, a fabric, a machine and a key holding
    /// only such keys. Written into the file and opened, as another version would leave them.
    Kept,
    /// Every value of both parts of every object set by command (`embroidery.set`, and
    /// `embroidery.setEntryExit` for the points), and then reset (`embroidery.reset`): the document
    /// must be the one it was, its file the same bytes.
    SetAndReset,
    /// The default fabric chosen by command (`embroidery.setup {fabric}`), its thread weight 40 and
    /// joining on, every machine value at the planner's default, and the built-in rows (135° on the
    /// page) as the document's `fillAngle`.
    DefaultFabric,
    /// Every object converted to manual stitches (`embroidery.convertToManual`, with the options
    /// the document is exported with): its stitches frozen as planned, sewn as they are, placed as
    /// the object was ([`converted`]).
    Converted,
    /// Every object converted, then restored (`embroidery.restoreObject`): the objects as they were,
    /// planned again ([`restored`]).
    ConvertedAndRestored,
}

/// `s`'s document with every converted object restored; a document with none is left as it was.
fn restored(mut s: Session) -> Session {
    run(&mut s, "select.all", json!({}));
    match s.execute("embroidery.restoreObject", &json!({})) {
        Ok(r) => assert!(r["restoredCount"].as_u64().unwrap() > 0 && r["editedCount"] == json!(0), "{r}"),
        Err(vectorcraft_engine::EngineError::Other(e)) => assert!(e.starts_with("nothing selected can be restored"), "{e}"),
        Err(e) => panic!("{e}"),
    }
    run(&mut s, "select.none", json!({}));
    s
}

/// `s`'s document with every object converted to manual stitches, planned with `options` (the
/// options it is exported with): what the convert passes over (stitches from a file, a symbol's
/// instance, type) stays as it is, and a document with nothing to convert is left as it was. Also
/// what it converted and passed over, as [`CONVERTED`] lists it.
fn converted(mut s: Session, options: &Value) -> (Session, String) {
    run(&mut s, "select.all", json!({}));
    let (count, reasons): (u64, Vec<String>) = match s.execute("embroidery.convertToManual", options) {
        Ok(r) => (
            r["convertedCount"].as_u64().unwrap(),
            r["skipped"].as_array().unwrap().iter().map(|k| k["reason"].as_str().unwrap().to_string()).collect(),
        ),
        Err(vectorcraft_engine::EngineError::Other(e)) => {
            assert!(e.starts_with("nothing selected can be converted"), "{e}");
            // Each object named with its reason: "12 (literal), 14 (notSewn)".
            (0, e.split('(').skip(1).filter_map(|r| r.split_once(')')).map(|(r, _)| r.to_string()).collect())
        }
        Err(e) => panic!("{e}"),
    };
    run(&mut s, "select.none", json!({}));
    let mut counted: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for r in reasons {
        *counted.entry(r).or_default() += 1;
    }
    let skipped: Vec<String> = counted.iter().map(|(r, n)| format!("{r} {n}")).collect();
    let summary = if skipped.is_empty() { format!("converted {count}") } else { format!("converted {count} · {}", skipped.join(", ")) };
    (s, summary)
}

/// What converting every object of each document converts and passes over, with why: pinned,
/// since an object that stops being converted (passed over as `unfreezable` or `insideObject`,
/// say) stays art that exports the same bytes, which no hash would show. The artwork's rows are
/// checked where its corpus is.
const CONVERTED: &str = "
fills  converted 5
strokes  converted 6
compound-path-with-a-hole  converted 2
clip-groups  converted 3 · clippingPath 2
symbols  converted 0 · symbolInstance 3
trim-after  converted 4
opened-design  converted 0 · literal 3
opened-design-clipped  converted 0 · clippingPath 1, literal 3
opened-design-cut-at-block-ends  converted 0 · literal 4
mixed-stitch-style-fill  converted 5
mixed-stitch-style-satin  converted 5
mixed-stitch-style-running  converted 5
mixed-every-option  converted 5
mixed-shrunk  converted 5
settings  converted 5
expanded-strokes  converted 3
exits  converted 4
stops  converted 3 · literal 4
capped-strokes  converted 7
rows-across-shapes  converted 6
rows-at-30  converted 6
bullseye-twemoji  converted 7
fox-twemoji.clipart  converted 7
heart-suit-twemoji.clipart  converted 1
joystick-twemoji  converted 7
pool-8-ball-twemoji  converted 3
puzzle-piece-twemoji  converted 1
smiling-face-with-smiling-eyes-twemoji.clipart  converted 6
sos-button-fluent.clipart  converted 2
video-game-twemoji  converted 14
watermelon-twemoji.clipart  converted 4
zebra-twemoji.clipart  converted 19
";

/// Each document's convert summary `now` (its name, then [`converted`]'s words) checked against
/// [`CONVERTED`]: a change lists each document that changed and prints the listing to paste.
/// Nothing pinned is passed over as `unfreezable` or `insideObject`.
fn check_converted(now: &[(String, String)]) {
    let recorded: Vec<(&str, &str)> = CONVERTED.lines().map(str::trim).filter_map(|l| l.split_once("  ")).collect();
    let changed: Vec<String> = now
        .iter()
        .filter(|(name, summary)| recorded.iter().find(|(n, _)| n == name).map(|(_, s)| *s) != Some(summary.as_str()))
        .map(|(name, summary)| format!("{name}: {summary}"))
        .collect();
    let listing: Vec<String> = now.iter().map(|(name, summary)| format!("{name}  {summary}")).collect();
    assert!(changed.is_empty(), "converted differently:\n{}\n\nlisting now:\n{}", changed.join("\n"), listing.join("\n"));
    for (name, summary) in now {
        assert!(!summary.contains("unfreezable") && !summary.contains("insideObject"), "{name}: {summary}");
    }
}

/// `s`'s document given `how`'s settings: a session holding it.
fn unchanging(mut s: Session, how: Unchanging) -> Session {
    match how {
        Unchanging::Kept => {
            let doc = s.doc().unwrap().doc.clone();
            let mut file: Value = serde_json::from_slice(&vectorcraft_format::save(&doc, false)).unwrap();
            // The marks a later version reads, of another shape than it reads (kept, read as unset):
            // Stop After, a kept original, and a run's mark, cuts and stops.
            let kept = json!({"fill": {"motif": {"name": "wave"}, "spacing": "auto"}, "stroke": {"points": {"rails": [[0.0, 0.0], [5.0, 5.0]]}}, "branch": true,
                "stop_after": "after", "original": {"frame": [1, 2], "node": 5}});
            let run_kept = json!({"manual": "yes", "trims": "all", "stops": [[0]], "stop_after": 1});
            each_node(&mut file["document"], &mut |node| {
                let e = node.as_object_mut().unwrap().entry("embroidery").or_insert_with(|| json!({}));
                if let Value::Object(e) = e {
                    for (k, v) in kept.as_object().unwrap() {
                        e.entry(k.clone()).or_insert_with(|| v.clone());
                    }
                    if let Some(Value::Object(run)) = e.get_mut("stitches") {
                        for (k, v) in run_kept.as_object().unwrap() {
                            run.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                    }
                }
            });
            let setup = file["document"].as_object_mut().unwrap().entry("embroidery").or_insert_with(|| json!({}));
            for (k, v) in [
                ("fabric", json!({"grain": 45, "tatami": {"curve": [1, 2]}})),
                ("machine", json!({"model": "x"})),
                ("hoop", json!(1)),
                ("fill_angle", json!("alongShape")),
            ] {
                setup.as_object_mut().unwrap().insert(k.to_string(), v);
            }
            let mut opened = Session::new();
            open(&mut opened, "kept.vectorcraft", &serde_json::to_vec(&file).unwrap());
            opened
        }
        Unchanging::SetAndReset => {
            let before = vectorcraft_format::save(&s.doc().unwrap().doc, false);
            let ids: Vec<u64> = s.doc().unwrap().doc.layers.iter().map(|l| l.id.0).collect();
            // Every value of both parts set by command on every object that takes them, an entry
            // and an exit on each, then every one reset.
            // Set: on every object that has the part (none, in a design of literal runs alone).
            let mut set = 0;
            for part in ["fill", "stroke"] {
                let all = json!({"ids": ids, "part": part, "stitch": "running", "spacing": 0.5, "length": 3, "angle": 30, "pull": [0.3, 0.1],
                    "pullPercent": 5, "push": 0.2, "expand": 0.3, "narrowAsSatin": 4, "underlay": "edge", "splitLength": 8});
                match s.execute("embroidery.set", &all) {
                    Ok(r) => {
                        assert!(r["changedCount"].as_u64().unwrap() > 0, "{part}: {r}");
                        set += 1;
                    }
                    Err(vectorcraft_engine::EngineError::Other(e)) => assert!(e.starts_with("none of these objects"), "{e}"),
                    Err(e) => panic!("{e}"),
                }
            }
            let mut objects = Vec::new();
            for layer in &s.doc().unwrap().doc.layers {
                layer.walk(&mut |n| objects.push(n.id.0));
            }
            let mut points = 0;
            for id in objects {
                for part in ["fill", "stroke"] {
                    // Refused on what takes no settings or has no such part.
                    points += usize::from(
                        s.execute("embroidery.setEntryExit", &json!({"id": id, "part": part, "entry": [10, 20], "exit": [30, 40]})).is_ok(),
                    );
                }
            }
            assert!(set == 0 || points > 0, "entry and exit points set where settings were");
            let keys: Vec<&str> = vectorcraft_doc::embroidery::PartKey::ALL.iter().map(|k| k.name()).collect();
            match s.execute("embroidery.reset", &json!({"ids": ids, "keys": keys})) {
                Ok(_) | Err(vectorcraft_engine::EngineError::Other(_)) => {}
                Err(e) => panic!("{e}"),
            }
            let after = vectorcraft_format::save(&s.doc().unwrap().doc, false);
            assert!(before == after, "set and reset: the file is not the one it was");
            s
        }
        Unchanging::Converted => converted(s, &json!({})).0,
        Unchanging::ConvertedAndRestored => restored(converted(s, &json!({})).0),
        Unchanging::DefaultFabric => {
            let fill = vectorcraft_stitch::plan::Params::for_style(vectorcraft_stitch::plan::Style::Fill);
            let setup = json!({
                "fabric": vectorcraft_stitch::fabric::DEFAULT_FABRIC, "threadWeight": vectorcraft_stitch::plan::THREAD_WEIGHT, "joinSameColor": true,
                "machine": {"jumpLength": fill.jump, "trimLength": fill.trim, "minStitch": fill.min_stitch, "splitLength": fill.max_satin},
                // The built-in rows given as the document's: the default, turned with each object as it is.
                "fillAngle": 180.0 - vectorcraft_stitch::plan::FILL_ANGLE,
            });
            run(&mut s, "embroidery.setup", setup);
            s
        }
    }
}

/// Call `f` on every node of a saved document's JSON (an object with an `id` and a `kind`).
fn each_node(v: &mut Value, f: &mut impl FnMut(&mut Value)) {
    if v.get("id").is_some_and(Value::is_u64) && v.get("kind").is_some() {
        f(v);
    }
    match v {
        Value::Object(m) => m.values_mut().for_each(|c| each_node(c, f)),
        Value::Array(a) => a.iter_mut().for_each(|c| each_node(c, f)),
        _ => {}
    }
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
            format.write(&Pattern { name: "threads".into(), blocks: blocks.collect(), ..Pattern::default() }).unwrap()
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
    // Every option under its earlier name, each keeping its earlier meaning: `stitchAngle` 30 is
    // the planner's clockwise 30°, `angle` 150 on the page, as the listing was recorded with.
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
        ("settings", settings(), json!({"length": 3, "jumpLength": 4})),
        ("expanded-strokes", expanded_strokes(), json!({})),
        ("exits", exits(), json!({})),
        ("stops", stops(), json!({})),
        ("capped-strokes", capped_strokes(), json!({})),
        ("rows-across-shapes", rows(json!("acrossShape")), json!({})),
        ("rows-at-30", rows(json!(30)), json!({})),
    ]
}

/// Fills whose rows the document sets (`embroidery.setup {fillAngle}`, the project owner's choices
/// of 2026-11-01: rows across the shape, a document setting): a tall bar, a wide one, one turned
/// 30°, an ellipse three times as long as wide, a disc and an L; with the rule across the shape
/// (each long fill's rows square to its length, the L's square to the diagonal its moments find it
/// 1.8 times as long along, the disc's fixed) or with 30°, turned with each object.
fn rows(fill_angle: Value) -> Session {
    let mut s = blank(300.0, 300.0);
    rect(&mut s, "#cc2222", 20.0, 20.0, 30.0, 120.0);
    rect(&mut s, "#2233cc", 70.0, 20.0, 120.0, 30.0);
    let turned = rect(&mut s, "#22aa44", 70.0, 80.0, 120.0, 30.0);
    run(&mut s, "object.rotate", json!({"ids": [turned], "angle": 30}));
    ellipse(&mut s, "#8822aa", 20.0, 160.0, 120.0, 40.0);
    ellipse(&mut s, "#ff8800", 180.0, 160.0, 60.0, 60.0);
    filled_mm(&mut s, "#552200", (180.0, 230.0), &[&[(0.0, 0.0), (8.0, 0.0), (8.0, 20.0), (24.0, 20.0), (24.0, 28.0), (0.0, 28.0)]]);
    run(&mut s, "embroidery.setup", json!({"fillAngle": fill_angle}));
    s
}

/// Satin strokes with caps (the project owner's choice, 2026-11-01, „Tak, wszędzie (polecam)”):
/// lines with a round, a square and a butt cap, sewn past their ends by half their width as each is
/// painted (the butt at its ends); an S curve with round caps; a closed ellipse and an open circle
/// whose ends meet, both round-capped, sewn with no cap (the project owner's choice, 2026-11-07,
/// „Nie, tam bez zakończeń (polecam)”); and a running line with round caps, which a running line
/// doesn't sew.
fn capped_strokes() -> Session {
    let mut s = blank(300.0, 300.0);
    let capped = |s: &mut Session, color: &str, weight: f64, d: &str, cap: &str| {
        let id = stroked(s, color, weight, d);
        run(s, "stroke.set", json!({"cap": cap, "ids": [id]}));
    };
    capped(&mut s, "#cc2222", 8.0, "M 30 30 L 200 30", "round");
    capped(&mut s, "#2233cc", 8.0, "M 30 60 L 200 60", "square");
    capped(&mut s, "#22aa44", 8.0, "M 30 90 L 200 90", "butt");
    capped(&mut s, "#8822aa", 10.0, "M 30 150 C 100 110 130 210 200 160", "round");
    let ring = ellipse(&mut s, "#ff8800", 40.0, 200.0, 80.0, 50.0);
    run(&mut s, "paint.setFill", json!({"none": true, "ids": [ring]}));
    run(&mut s, "paint.setStroke", json!({"color": "#ff8800", "ids": [ring]}));
    run(&mut s, "stroke.set", json!({"weight": 6.0, "cap": "round", "ids": [ring]}));
    capped(&mut s, "#552200", 6.0, "M 190 250 A 30 30 0 1 1 190.001 250", "round");
    capped(&mut s, "#2233cc", 1.0, "M 30 280 L 200 280", "round");
    s
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
/// rotated and scaled (its square's rows, at no angle of its own, turned with it).
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
    let pattern = Pattern { name: "cuts".into(), blocks: vec![a, b], ..Pattern::default() };
    let mut s = Session::new();
    open(&mut s, "cuts.pes", &Format::Pes.write(&pattern).unwrap());
    s
}

/// The pinned stops document's exports open again with each stop, in each format that names its
/// threads (the stops a file writes as changes to the same thread read back as stops), and are
/// written back to the same bytes in PES, PEC and JEF. VP3 holds no cut beside a change of thread,
/// so it reads those as nothing and is written the same from its first writing back on.
#[test]
fn the_stops_exports_open_and_write_back_unchanged() {
    let mut s = stops();
    let planned = run(&mut s, "embroidery.stats", json!({}))["stops"].clone();
    assert!(planned.as_u64().is_some_and(|n| n > 0));
    let reopened = |format: Format, bytes: &[u8]| {
        let mut back = Session::new();
        open(&mut back, &format!("stops.{}", format.extension()), bytes);
        assert_eq!(run(&mut back, "embroidery.stats", json!({}))["stops"], planned, "{format:?}: its stops");
        export(&mut back, format, &json!({})).unwrap()
    };
    for format in [Format::Pes, Format::Pec, Format::Jef, Format::Vp3] {
        let bytes = export(&mut s, format, &json!({})).unwrap();
        let again = reopened(format, &bytes);
        match format {
            Format::Vp3 => assert!(reopened(format, &again) == again, "VP3: written the same from its first writing back"),
            _ => assert!(again == bytes, "{format:?}: written back to the same bytes"),
        }
    }
}

/// Stops, each written as a change to the same thread: a design written in code with a stop
/// between two needle points of its first block and one at that block's end (the thread change's,
/// written as nothing), opened from PES, so that the stop reads back as one; two squares of one
/// thread far apart, the first stopped after (Stop After), and a line stopped and cut after as the
/// design's last object (its stop the end's, written as nothing); and a run of three subpaths, the
/// thread cut before the third, a stop inside the first and one at the end of the second.
fn stops() -> Session {
    use vectorcraft_doc::embroidery::{Embroidery, StitchRun};
    let line = |y: f64, from: i32, to: i32| (from..to).map(move |i| Stitch::new(f64::from(i) * 2.0, y, Command::Stitch));
    let mut a = Block::new(Thread::rgb([200, 0, 0]));
    a.stitches.extend(line(0.0, 0, 5));
    a.stitches.push(Stitch::new(8.0, 0.0, Command::Stop));
    a.stitches.extend(line(0.0, 5, 10));
    a.stitches.push(Stitch::new(18.0, 0.0, Command::Stop));
    let mut b = Block::new(Thread::rgb([0, 0, 200]));
    b.stitches.extend(line(10.0, 0, 10));
    let pattern = Pattern { name: "stops".into(), blocks: vec![a, b], ..Pattern::default() };
    let mut s = Session::new();
    open(&mut s, "stops.pes", &Format::Pes.write(&pattern).unwrap());
    run(&mut s, "file.info", json!({"title": "stops"}));
    let first = rect(&mut s, "#cc2222", 20.0, 60.0, 20.0, 20.0);
    rect(&mut s, "#cc2222", 150.0, 60.0, 20.0, 20.0);
    run(&mut s, "embroidery.setStopAfter", json!({"ids": [first]}));
    let last = stroked(&mut s, "#2233cc", 1.0, "M 20 120 L 150 120");
    run(&mut s, "embroidery.setStopAfter", json!({"ids": [last]}));
    run(&mut s, "embroidery.setTrimAfter", json!({"ids": [last]}));
    s.edit("Add Run", |d, _| {
        let layer = d.layers.first().map(|l| l.id);
        let id = d.alloc_id();
        let mut bp = vectorcraft_geom::BezPath::new();
        for (k, y) in [150.0, 160.0, 170.0].into_iter().enumerate() {
            let x0 = 20.0 + 60.0 * k as f64;
            bp.move_to((x0, y));
            for i in 1..5 {
                bp.line_to((x0 + 6.0 * f64::from(i), y));
            }
        }
        let paint = vectorcraft_color::Paint::solid(vectorcraft_color::Color::rgb8(0x22, 0x99, 0x44));
        let mut node = vectorcraft_doc::Node::path(
            id,
            vectorcraft_geom::PathData::from_bezpath(&bp),
            vectorcraft_doc::Appearance::basic(vectorcraft_color::Paint::None, paint, 1.0),
        );
        let place = vectorcraft_doc::embroidery::Mark::place;
        let marks = StitchRun { trims: vec![place(2, 0)], stops: vec![place(0, 2), place(1, 4)], ..StitchRun::default() };
        node.extras_mut().embroidery = Some(Box::new(Embroidery { stitches: Some(marks), ..Embroidery::default() }));
        d.insert(layer, usize::MAX, node).map_err(|e| vectorcraft_engine::EngineError::Other(e.to_string()))?;
        Ok(())
    })
    .unwrap();
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

/// [`mixed`] with settings at every level: on objects (a fill's angle, pull and underlay, a stroke
/// sewn as a running line, a stroke's split and push, a fill left unsewn, an entry point), in the
/// document (the fleece fabric, a thread weight, the machine's satin split) and, in the listing,
/// options given to the export (a stitch length, a jump length).
fn settings() -> Session {
    let mut s = mixed();
    let ids: Vec<u64> = s.doc().unwrap().doc.layers[0].children().unwrap().iter().map(|c| c.id.0).collect();
    let [rect, letter, satin, zigzag, both] = ids[..] else { panic!("mixed holds five objects: {ids:?}") };
    run(&mut s, "embroidery.set", json!({"ids": [rect], "part": "fill", "angle": 30, "pull": [0.3, 0.1], "underlay": "edge"}));
    run(&mut s, "embroidery.set", json!({"ids": [satin], "part": "stroke", "splitLength": 5, "push": 0.4}));
    run(&mut s, "embroidery.set", json!({"ids": [zigzag], "part": "stroke", "stitch": "satin"}));
    run(&mut s, "embroidery.set", json!({"ids": [both], "part": "fill", "stitch": "none"}));
    run(&mut s, "embroidery.setEntryExit", json!({"id": letter, "part": "fill", "entry": [200, 120]}));
    run(&mut s, "embroidery.setup", json!({"fabric": "fleece", "threadWeight": 60, "machine": {"splitLength": 7}}));
    s
}

/// Objects whose appearance the walk expands, with strokes: a filled rectangle with a satin stroke
/// and a running one (each sewn along the path), a rectangle whose upper stroke is roughened (sewn
/// along its roughened path: Expand Appearance leaves it a stroke, the project owner's choice of
/// 2026-11-02; before, the area it paints), and a zigzag of two strokes set to running (two lines
/// along it).
fn expanded_strokes() -> Session {
    let mut s = blank(300.0, 300.0);
    let second_stroke = |s: &mut Session, id: u64, weight: f64| {
        run(s, "appearance.addStroke", json!({"ids": [id]}));
        s.edit("Second Stroke", |d, _| {
            if let Some(vectorcraft_doc::AppearanceItem::Stroke(st)) =
                d.node_mut(vectorcraft_doc::NodeId(id)).and_then(|n| n.appearance.items.last_mut())
            {
                st.width = weight;
            }
            Ok(())
        })
        .unwrap();
    };
    let both = rect(&mut s, "#cc2222", 20.0, 20.0, 80.0, 60.0);
    run(&mut s, "paint.setStroke", json!({"color": "#2233cc", "ids": [both]}));
    run(&mut s, "stroke.set", json!({"weight": 6.0, "ids": [both]}));
    second_stroke(&mut s, both, 1.0);
    let roughened = rect(&mut s, "#22aa44", 140.0, 20.0, 80.0, 60.0);
    run(&mut s, "paint.setStroke", json!({"color": "#8822aa", "ids": [roughened]}));
    run(&mut s, "stroke.set", json!({"weight": 4.0, "ids": [roughened]}));
    second_stroke(&mut s, roughened, 4.0);
    run(&mut s, "effect.apply", json!({"effect": "distort.roughen", "ids": [roughened], "item": 2}));
    stroked(&mut s, "#ff8800", 4.0, "M 20 200 L 60 160 L 100 200 L 140 160 L 180 200");
    let zigzag = s.doc().unwrap().doc.layers[0].children().unwrap().last().unwrap().id.0;
    second_stroke(&mut s, zigzag, 1.0);
    run(&mut s, "embroidery.set", json!({"ids": [zigzag], "part": "stroke", "stitch": "running"}));
    s
}

/// Entry and exit points the planner sews toward: a rectangle that ends at an exit on its right
/// side, a running line whose exit lies at its start (sewn from its end, reversed), a satin line
/// 6 mm wide entered at its left end and leaving at its right (its underlay ending it back at the
/// left, a pass along its middle turns it round), and a running loop carried on round to its exit.
fn exits() -> Session {
    let mut s = blank(300.0, 300.0);
    let fill = rect(&mut s, "#cc2222", 20.0, 20.0, 80.0, 50.0);
    run(&mut s, "embroidery.setEntryExit", json!({"id": fill, "part": "fill", "exit": [100, 45]}));
    let line = stroked(&mut s, "#22aa44", 1.0, "M 130 30 L 270 30");
    run(&mut s, "embroidery.setEntryExit", json!({"id": line, "part": "stroke", "exit": [130, 30]}));
    // 6 mm wide: its underlay (an edge run and a zigzag) ends it back at its left end.
    let satin = stroked(&mut s, "#2233cc", 6.0 * PT_PER_MM, "M 20 120 L 200 120");
    run(&mut s, "embroidery.setEntryExit", json!({"id": satin, "part": "stroke", "entry": [20, 120], "exit": [200, 120]}));
    let ring = ellipse(&mut s, "#8822aa", 40.0, 170.0, 120.0, 60.0);
    run(&mut s, "paint.setFill", json!({"none": true, "ids": [ring]}));
    run(&mut s, "paint.setStroke", json!({"color": "#8822aa", "ids": [ring]}));
    run(&mut s, "stroke.set", json!({"weight": 1.0, "ids": [ring]}));
    run(&mut s, "embroidery.setEntryExit", json!({"id": ring, "part": "stroke", "exit": [160, 200]}));
    s
}

/// The exits document's units meet the points they were planned toward, within the Check tab's
/// 1 mm (`POINT_MISS_MM`): each part sewn as it is asked to, not only as it was recorded.
#[test]
fn the_exits_document_meets_its_points() {
    let mut s = exits();
    let order = run(&mut s, "embroidery.sequence", json!({}));
    let units: Vec<&Value> = order["blocks"].as_array().unwrap().iter().flat_map(|b| b["units"].as_array().unwrap()).collect();
    assert_eq!(units.len(), 4, "{order}");
    for u in units {
        for point in ["entry", "exit"] {
            if let Some(missed) = u["missedBy"][point].as_f64() {
                assert!(missed <= vectorcraft_engine::cmd::embroidery::POINT_MISS_MM, "unit {}'s {point} missed by {missed} mm", u["unit"]);
            }
        }
        assert!(u["missedBy"]["exit"].is_number(), "unit {} was planned toward an exit", u["unit"]);
    }
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
/// unselected → its id.
fn stroked(s: &mut Session, color: &str, weight: f64, d: &str) -> u64 {
    run(s, "paint.setFill", json!({"none": true}));
    run(s, "paint.setStroke", json!({"color": color}));
    let id = id(&run(s, "path.create", json!({"d": d})));
    run(s, "stroke.set", json!({"weight": weight, "ids": [id]}));
    run(s, "select.none", json!({}));
    id
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
