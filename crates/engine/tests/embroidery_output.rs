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
bfd49c35294bb3c75e4889751e40a31aeb418828c275aff5fabd9b8d80db0cde  fills.dst
d83d6fb4a1b0e263f6052c51118b8f980c90ab482f766d0bab5f77957f779854  fills.tap
d83d6fb4a1b0e263f6052c51118b8f980c90ab482f766d0bab5f77957f779854  fills.t01
1b21849d5d0bd289963b854b283c661112a539449ccf088de7f0f0bfd85aecbb  fills.exp
03539b9e7fbf1ea340f29c0eb9d61c944bdc7df5ca5d13b43c7450a961b17aa3  fills.pes
dd984043c9f8e90f5785ee5e15e8864268a8690e32ad6b122986307e7b1dc1ff  fills.pec
55f71114cad5eb31a2b4013aeef70790b359a68e8d3a04aef15d6364a6ad1ccd  fills.jef
f0b126586ad743cdd15a8399941dc3fd806ebaa02d2e2c69e957acd9cc95625e  fills.vp3
39797e76b30fbc5590344f74d78f56d2df5650b568fa19e4524b51e9bf735a81  strokes.dst
f836d6d6fd8511fad23882411900ce7fd44c47dd9f9f89afb7735bc21c51aaa4  strokes.tap
f836d6d6fd8511fad23882411900ce7fd44c47dd9f9f89afb7735bc21c51aaa4  strokes.t01
eb533088cadf0e803acb15d5346d50cb1689df0c0a95b151ca3072c5c713cdd0  strokes.exp
36a596ccfaf7b525a17aa7c501657b86dd6a54a6e073e02c50bf3c97a3290703  strokes.pes
85d22fedc8cb41e60c101a3950bfbd9961ade8f731b08b227988123ab7a6a197  strokes.pec
89335107460998a8cc4a8d1ce76ea5b47dad41a5602e00b323e2e8a2bb68181a  strokes.jef
88d25ea4c0d4a14088d77d6557ecc70181aef075d591b7add39ea17a5f015e17  strokes.vp3
eff5ce99a798abd87bdfe0de5afeaf2bef01524734adb05677df70d77735785b  compound-path-with-a-hole.dst
0abed765c7047a8983dcaf3db08525f38dfec3d60efacfb8d88fb6ce8a102763  compound-path-with-a-hole.tap
0abed765c7047a8983dcaf3db08525f38dfec3d60efacfb8d88fb6ce8a102763  compound-path-with-a-hole.t01
183875dd627a5389393fb37d161a83b72cac9707ec4a1c3367bef1fa774289a6  compound-path-with-a-hole.exp
3f1c4dcb8b4253521a930b2eb4c356198ee441bb66634f8ac28578e7661c5bf0  compound-path-with-a-hole.pes
c9be8a855c8ea00349158ba21026a4476473c06f9b9882ec65f389237a35c7c6  compound-path-with-a-hole.pec
de2e37a82b2f7f53f991261962e202e182e8af819ca154a80c63a4732c7af32c  compound-path-with-a-hole.jef
8c572a204b7b9368a49535f701f2df48965be3cbd9e74ee3d9d197c97ea2c86a  compound-path-with-a-hole.vp3
9f81cb7eda102267fbaeb06f45013f5c0c493dec3e53adbb297a1d59037143ac  clip-groups.dst
a5ff05ef476f74540b5d4696b7dc99189a2aa7fecf745dc6b49f8aaf0e28e0b0  clip-groups.tap
a5ff05ef476f74540b5d4696b7dc99189a2aa7fecf745dc6b49f8aaf0e28e0b0  clip-groups.t01
0d521a9d17424dc7930e42eeed86f6c9028c7fd2f31fccdc904d0411c4e14862  clip-groups.exp
8896f294fd6dbbdf6b0782c73d50002af699916b10dd9957c9750303db087737  clip-groups.pes
52f4da01db5bc485a51eccf305638f428c847ba0ed34911a04d0c1a35b4d3083  clip-groups.pec
36978e02311698098b63d2760f90a7f414d56349a4a54d3cb412a45078e095f1  clip-groups.jef
cde69313d5b4d2bfa3d96d4a03e174d067e467fd52e5bf29f98f0551814d9cbc  clip-groups.vp3
f8b68f2f053586b5ba1157d727992e7da6d292fba40a8dc1f16a7c31bd272d53  symbols.dst
46ac4ca5c04e5ed49f57462bfd4708c747788fc7488988f38554ad93fe4240db  symbols.tap
46ac4ca5c04e5ed49f57462bfd4708c747788fc7488988f38554ad93fe4240db  symbols.t01
bbb4059dc3f24cd91aad3a7c836b9da1c5c39c88f761c362cc2f85e0e3eaf723  symbols.exp
3c44b14027f8903cd455cc0f0ae41a05b0ace683181d5fc3d49b260d3442c3a3  symbols.pes
0b56837c1cfa6afd0c196bfa2f55371916c2c7c4eb69e98d06b45612137f439d  symbols.pec
f0613950519a1d81612a27ad31cb9e075d37e38806032c768db650dbeff17b79  symbols.jef
21f774fa4834cfe69df807dd17dd7950992133aad84cd9a6b6aab97a91aac539  symbols.vp3
acc376edb11b4c6fda9bc7a66d9f5364395b3091ba9675c511123d801eb76ace  trim-after.dst
379c27976c8dd221c00526da6fcb7cefbfb04c74d07bf14be68cab0ee933427e  trim-after.tap
379c27976c8dd221c00526da6fcb7cefbfb04c74d07bf14be68cab0ee933427e  trim-after.t01
0a5451798049d733bc66859d09742f35fac1f02115c90ca09d8f7ae6fb15937d  trim-after.exp
efea759a00d4625d06bd982d1d13e6a41252838a7aa86392deb7d68627d88b7d  trim-after.pes
fc82fa6b1f72ef26b1c1b0c6f01723ffa18dafe2ca5d42cdc312ef454b9ecdad  trim-after.pec
bc1ad67e7e327908887b9625e781abeb3565b3fb99385d4126b1705d8ec0d304  trim-after.jef
12d963aa3794c56549c49b0f3f6c2ce1d54add0d472e5bbcce3d614ef5fbbe12  trim-after.vp3
8c0006fb0623ead67b19611f8dbefda2fc98f2a69b30e380292c8067d07b23e8  opened-design.dst
7a98e9d31a3bb5646e99e0463e02352cbb42234f292e2529e00e9d8a599dc440  opened-design.tap
7a98e9d31a3bb5646e99e0463e02352cbb42234f292e2529e00e9d8a599dc440  opened-design.t01
2398a03de7591f9d4dcf59624b172ec2160eb86b40b0186ef347855e814c5028  opened-design.exp
af25aea00613c4c5e52657c8677a46ed0888a50a97aa4eff6ca3094feb465500  opened-design.pes
0f77a369cf875de64570af8ae9c18ed74e2650bc9f156c31a05c7a6057a40d27  opened-design.pec
fff7d43d2e75987e2ac5308fdf631af70551b0d0e73b52addf5ca10081ae2865  opened-design.jef
c1c7cf69785b7865188b78ef6ce2194c73c02839520f068732d69be52e29ab21  opened-design.vp3
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
16d22beabdd60cad0d9eeb7b30c30cf71eb8f198edad1117bae7727286f776b4  mixed-stitch-style-fill.dst
e751bbb64713b8fbbc72a5f69b248ed7ce53a7325fec738a3c7247980f4ae2cc  mixed-stitch-style-fill.tap
e751bbb64713b8fbbc72a5f69b248ed7ce53a7325fec738a3c7247980f4ae2cc  mixed-stitch-style-fill.t01
576f3c8ff3e6b9b9bc099cd6ac2ccdde421c290614875c43c4cd1e8368b2eae1  mixed-stitch-style-fill.exp
d2061f444d93cfd18e30c7cb028f8b599232c190b6f01ef9314bb7c627ba10bf  mixed-stitch-style-fill.pes
9350de5dbaad6e73e9403fa910b9fbda5929fe44704e238fe408d5ccb3ad172b  mixed-stitch-style-fill.pec
1334231fa48788fc53b9b59f2904b2e5eeae3468f89c80a94e2cfd188bb9d6c2  mixed-stitch-style-fill.jef
bc6837faa07a1e8bb2cf4267b59dc67a16e40fd83cb201065ce719c724153602  mixed-stitch-style-fill.vp3
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
a8cb4189cee41465a25d1a4d8191cc122b302ae5289a2150e3b4aca03d7aa049  mixed-shrunk.dst
3e4baff4739859b1ade5923c927bae77fc823c45082d9f1645fbb882716297ca  mixed-shrunk.tap
3e4baff4739859b1ade5923c927bae77fc823c45082d9f1645fbb882716297ca  mixed-shrunk.t01
a785608b345ea6ed9039e9d2db3ea25883d022539fbaeb69c6d6943d988e5248  mixed-shrunk.exp
cf022f2b570ef05b9940d64c49219c013c7ee80b5ac0dc33e8b3d84453edd5ad  mixed-shrunk.pes
027cea918ef3dc5b2c52850e8091be257377c075041287de86bed6ebc3b0c629  mixed-shrunk.pec
e488e732b9fb9f7d1a68872d0bc3d79de3165f50df07bae429220b2761ff57a5  mixed-shrunk.jef
e8fc1c3736b9371fad3f6b129d07262de075693622014418ac0c7f241480aadb  mixed-shrunk.vp3
";

/// The real designs' artwork at the person's size ([`DESIGNS`]), listed as [`SYNTHETIC`] is.
const ARTWORK: &str = "
236957804f516fe0004917fc484b5da2b1daf8f9f6f09061ce353f9684a7235e  bullseye-twemoji.dst
e280bb5748b1b92274b813cbbf961269a79cb37c4ac59ed4d7af16b393416b32  bullseye-twemoji.tap
e280bb5748b1b92274b813cbbf961269a79cb37c4ac59ed4d7af16b393416b32  bullseye-twemoji.t01
ad3aaf8945c67d9e706a3b47d230611f186f4d69190662fa18f9c5df818c5f0d  bullseye-twemoji.exp
467bff688288710650df332d4dcb92e8a4ff8ce6b33521d8b85dcf8946dbf545  bullseye-twemoji.pes
e28af61cc1acdd408c4562dc6a964cee1c085103f55133c3a4249b400affc77c  bullseye-twemoji.pec
523dbf9a258e8b259b1698e53c337a86d55306d05e410c1b80cd392e08495d12  bullseye-twemoji.jef
e0ac16c407b086bd1c964569b655b2b33ae7daa05689e78f9fecc1e708c4b618  bullseye-twemoji.vp3
beae2e150caf987f5a39397dbef9238bf14f09acd707e9d18d139928b579cbad  fox-twemoji.clipart.dst
2b0c4f2447243770c7215c90de2dd376dd0553501399e1ab059184272107837d  fox-twemoji.clipart.tap
2b0c4f2447243770c7215c90de2dd376dd0553501399e1ab059184272107837d  fox-twemoji.clipart.t01
1e3b5433c82b1dddd0b574f34012c4d9d329989002f9aedc016ef92ae3c2c4be  fox-twemoji.clipart.exp
2ee47aa9f7dd1002a1b4d5c36b07274cefd738bd6991e383ffa931ee64b8bb6a  fox-twemoji.clipart.pes
85dbeb2226be44505aecbed7568227b742e0f70dc5f7443ca7864e2b4af645b8  fox-twemoji.clipart.pec
f069741c54bbc08d740a4d28f9cc8888b260f32f0dcf3d576748b979e13be899  fox-twemoji.clipart.jef
8f1a10785bde9a48aeee19350f2d64f068e89cde053a982d7ef5985fc8e7b62b  fox-twemoji.clipart.vp3
a0465592a39b91b6be9070b7f5205361db9ff5a3e33d4593a6ad86d931c44f33  heart-suit-twemoji.clipart.dst
9e25d01d186774e3eec73b4427abb21edec84541e15f0cb15f82593616bea9a5  heart-suit-twemoji.clipart.tap
9e25d01d186774e3eec73b4427abb21edec84541e15f0cb15f82593616bea9a5  heart-suit-twemoji.clipart.t01
0d0e7e1d7d20e976a08fafd3ed6df5e8806a7e4e572a3513ee14e9d4e849fa64  heart-suit-twemoji.clipart.exp
0986deff7fa231d01098344499c2f0a531b2cdf94c3c88392182a962dc3fbefa  heart-suit-twemoji.clipart.pes
d2c0daeed2ac136d8c68d85c09212948c28563b0b245100defb688ab30a35852  heart-suit-twemoji.clipart.pec
dfbaebe23679c276c14ebf06d8d056b5bb5a9ccea53d08967bdbe449f2504797  heart-suit-twemoji.clipart.jef
9ac180717f459d64e1de2056683fd228a91187f008ca041bb35b343b2bb8bea9  heart-suit-twemoji.clipart.vp3
41cdd3a94031c2ced989b08d7a5bc2e35567ed7b3d331a58c19229832b9bd0ff  joystick-twemoji.dst
8f763b55e74106ddd2a68657ede308f5eb61e59775e2e8c7575053d0038880a5  joystick-twemoji.tap
8f763b55e74106ddd2a68657ede308f5eb61e59775e2e8c7575053d0038880a5  joystick-twemoji.t01
5336690149a2f03441610ba3e0f5376b92d62914f4deec1afc33a2a23c1a724b  joystick-twemoji.exp
5892e82bc9c6be6060aa86fbd36304aca37fbeae70b1bfda5158d18beb649d41  joystick-twemoji.pes
9fe0186a0c04c91e1a68f413ad53149b8257a94312ccf518591e1b49d4d025e1  joystick-twemoji.pec
8012d1a66a7c2b4702e585c2939ddb82f5529c58362468e7731a87590a60ee90  joystick-twemoji.jef
e32c85414608c0999f028664f71e4c298b689dc509b38e1c234d2791d70556fb  joystick-twemoji.vp3
897fd9a5da0fd317c70e054f762a2bbee2a24e83fd36962862ef9427f887531c  pool-8-ball-twemoji.dst
c838575586b1994b4d907091497e585f5d270e05b85e9a264a90704d3f9ae383  pool-8-ball-twemoji.tap
c838575586b1994b4d907091497e585f5d270e05b85e9a264a90704d3f9ae383  pool-8-ball-twemoji.t01
788d28076f8b3381bbc800b13959eb855ee70ffb4f4c0ba156628315796d400f  pool-8-ball-twemoji.exp
95698fa0f5b71adc2276be56d9cf8ae16835afe521ec9f7a0aec91ecc1538d42  pool-8-ball-twemoji.pes
b024631ac32884dbfcf25f9db97506f648dd512b893b51f674161081769a05b8  pool-8-ball-twemoji.pec
dd93f048a375e0a5f66067d7cdf5ebb891fb607eb57884741f5bb2c9aac10f06  pool-8-ball-twemoji.jef
d46fe82e3fcfdcfb5b2660c5bb9c4ed11ef5e49554723560b122b0eef442525f  pool-8-ball-twemoji.vp3
2745472aa7ecb5cf1b462edcc2726c589f298b36621bd65c0f3f40f6a2c161c5  puzzle-piece-twemoji.dst
ea063b2fd303dd23756ec75d633a0c1d6bd1e04d0040515bab5374b4829583fe  puzzle-piece-twemoji.tap
ea063b2fd303dd23756ec75d633a0c1d6bd1e04d0040515bab5374b4829583fe  puzzle-piece-twemoji.t01
1d333d7bcc145493c77ccbafe909d23ab03f4f4e7ce766bab9078373d733fe98  puzzle-piece-twemoji.exp
fdcf0af314400b90877fc07cd4bf2be34587c65719767b338aceb2703972f99a  puzzle-piece-twemoji.pes
99239d1d9fd27161ab6050dd4df592c69545fa0c008bcefbfc30798a7151fd7f  puzzle-piece-twemoji.pec
f4aa4bac74bcac57a19f1c5368a7358ae049fcd417e82ecdb12fee6ebe104a2c  puzzle-piece-twemoji.jef
34b8fc99ca073fda7681b6e0c8f35e4d54873bb71580f60e173293609220bf8d  puzzle-piece-twemoji.vp3
1db23447b17b5df2ff6c898df0b0a70f46fa37b277f8ad948ac4398d2457f9de  smiling-face-with-smiling-eyes-twemoji.clipart.dst
14abcb883c9bb68fd32733468f4f71e577b1c4d3c95a4b55ece840cd6cdcacfe  smiling-face-with-smiling-eyes-twemoji.clipart.tap
14abcb883c9bb68fd32733468f4f71e577b1c4d3c95a4b55ece840cd6cdcacfe  smiling-face-with-smiling-eyes-twemoji.clipart.t01
19f60296fe21f16e10bc737bef1d821af9150f777841ead64c11af210c0d799a  smiling-face-with-smiling-eyes-twemoji.clipart.exp
4a3f0f02b18c68b51a6012b8414c72b002f51cc9a55756658aa1320445e06b83  smiling-face-with-smiling-eyes-twemoji.clipart.pes
b7dc5d51320bd367150ebd1395ee821f439ad7946022b79aee9df74ee6f9c80d  smiling-face-with-smiling-eyes-twemoji.clipart.pec
9052b32dae1382a6f2156e509bdf994dbaf63ae1852bc8538fc31c689ce63d52  smiling-face-with-smiling-eyes-twemoji.clipart.jef
018378309cc8904355ebb97e122b1626198b29dbc5f8c537133a201582e77019  smiling-face-with-smiling-eyes-twemoji.clipart.vp3
e556ec1fbb766bc1cfef7790f7307ebc94d8e5ff5662ddc05075e31254a36841  sos-button-fluent.clipart.dst
7b85ad77234d9b9ce29bfabc8cd0d3b8b549fb52f60baf10fad907e1a7b895b5  sos-button-fluent.clipart.tap
7b85ad77234d9b9ce29bfabc8cd0d3b8b549fb52f60baf10fad907e1a7b895b5  sos-button-fluent.clipart.t01
52951b7932fbe7af736f63ed229ef21dda5a5867bc3e32c90ab6fe20ce93fe6e  sos-button-fluent.clipart.exp
0c6d0f065557dffaac67fd6db943b7735dfff7c4517fca6be67dce1344ee5529  sos-button-fluent.clipart.pes
b3ea1d943f382921bb4ed29c7f712656544c525ad8701d4972dacbac6cd59294  sos-button-fluent.clipart.pec
05147fb7c1f075fb2c46c49e227aacc233aa56acd2501f7b33c7ffd89555b260  sos-button-fluent.clipart.jef
e963cbf7a923eee89e621cc40d4058c751b64e47df05aa68a416c1152cefdaaa  sos-button-fluent.clipart.vp3
61cd25c0894c0784312870219a56001d37a58d1629ad9fe12a40590067d0f5a1  video-game-twemoji.dst
24dec2e0a353ca02b61d8691dbd5388f3bab86e8a5d6b964c90355dfa560a268  video-game-twemoji.tap
24dec2e0a353ca02b61d8691dbd5388f3bab86e8a5d6b964c90355dfa560a268  video-game-twemoji.t01
4576f05b0aca40b2fb97375a0fb12bb47b6661774047b85926da715c32e55e9f  video-game-twemoji.exp
5b60086836be865519df4b0ceee9775db2359cee15e2b4077de600c497fa8282  video-game-twemoji.pes
ed9705aa27c8c699b4635f495568adecf071702afdd44ab64314e8551df52337  video-game-twemoji.pec
3c1a16ea40fdb5089ebd6da6b2ff3a74008879ad59ff4fb140f5968f8eacebc1  video-game-twemoji.jef
d86211d4d504372840fa0ae1d22f33de8684ebfd2a589bd2d91152c718f4152d  video-game-twemoji.vp3
f4c151c3583b0fee92777903df59e82a837b981835dbb8d9219cde0096bdc9b3  watermelon-twemoji.clipart.dst
83dbc939ff28097a39c8976f166757ee58ffb6f28a657d2373567a82d63008ae  watermelon-twemoji.clipart.tap
83dbc939ff28097a39c8976f166757ee58ffb6f28a657d2373567a82d63008ae  watermelon-twemoji.clipart.t01
8b245eb245b1bf1821e04514a7914b4a4b3f9a60c8488f403cffd78894e9e758  watermelon-twemoji.clipart.exp
8ce685df0a530ecb72bd68d87c6bb52e5430e63a21dcdba69068f4cdeb643dac  watermelon-twemoji.clipart.pes
88dc2ec1c3f51a14eb38e598c53dbc8f9151b1735c60d917a36d90d77221fdb3  watermelon-twemoji.clipart.pec
8e3e9895af14cb9352257e6e726712074b4634ed58ea41e3430a0e8b69872973  watermelon-twemoji.clipart.jef
30182404b39ec0494f7a52d9fa66ca5d67c7843990413059d4f6a91da8bedd98  watermelon-twemoji.clipart.vp3
ae9dd329a4ceb90e69f806d7aaa0473ad092f0d53b33aee877b1156bd895d266  zebra-twemoji.clipart.dst
50af3d41ebd4f210ad87dd0c5a70b8579857c816a04dc248e0f51e0dfde25e2d  zebra-twemoji.clipart.tap
50af3d41ebd4f210ad87dd0c5a70b8579857c816a04dc248e0f51e0dfde25e2d  zebra-twemoji.clipart.t01
a78b747fe515ebd5ae70a47fb505068878aa7c5c61f62173a902e6a706edaa33  zebra-twemoji.clipart.exp
9653679744b38e251e4c2634b6d80821be40eed1148048ffeb1dec9426369863  zebra-twemoji.clipart.pes
eb752886eeeb6d4e3d74468f26ca832c27aad23a244e2a6e0eb84deeccc312f5  zebra-twemoji.clipart.pec
f7dcc4f7d05fbb6dede6397385494dd37944ca02d152a5235c3ae5246242ca6a  zebra-twemoji.clipart.jef
5b6bce2b60bd703cd1362607f1bfdb95579b7b14c137345473bf9ccd6a5d645f  zebra-twemoji.clipart.vp3
";

/// The PES, PEC and JEF exports of [`SYNTHETIC`] apart from their thread numbers ([`threads_apart`]):
/// each one's hash with its thread numbers blanked, its name, and the numbers, in sewing order.
const SYNTHETIC_THREADS: &str = "
3fcd799b6d3cb2ea07f6024394a08b32059d60fcd2764ef3f17b7fdff35bbbf1  fills.pes  5 21 37 5
0d8637177ce9163edb14fe2a72d7e69f347cc6e03dd6962a2836db4b387f5eab  fills.pec  5 21 37 5
2e82b7672be2c74e2468336deb2c2205031f424ea0ba58e1297860bf36510d48  fills.jef  33 59 45 33
c3a06ac48968baf33f01b1cef11bf68f9a1ee1d24f5ceb5bb3259e5522aa1bb8  strokes.pes  5 21 7 37 52 24
117af9ccdd3e6d987902d27c05235b4c30b29e1832d3951d491e31273103b074  strokes.pec  5 21 7 37 52 24
5d7230325fc878bdc57c4baa2be365a2062165cfe894e579b7a3edf4234b2aa0  strokes.jef  33 59 59 45 71 14
7f3afc7ad0410d2a5b493bee6f4ae5ce7d2df0041b68a243fd5147ead2859f1d  compound-path-with-a-hole.pes  5 21
0e58eb05263636b0de5a1e00399ae071fedc07c43aaf8f5ab2768fae12ac07e9  compound-path-with-a-hole.pec  5 21
a5d1469fdd76d6e7bf8fef8639ea3951796726bc0cc0b287a903b98cff897202  compound-path-with-a-hole.jef  33 59
0a272050ed47222857ff8740cc57ec24c6a4fc289ba3ce30d66c25bd109d086d  clip-groups.pes  58 5 21 37 23
f9a66e5486b9f056151931e79c3c8a86da828c6f00d7fc4064872f8fd2b5d2fb  clip-groups.pec  58 5 21 37 23
3b5d9ab166760d963e13d8555da6a36b4a2b8c69cc73e7f320bdc65a21b28682  clip-groups.jef  37 33 59 45 1
a8b7e0c436518aa8abf5e2b4332787ab3400a7d3ace5eaaaac9346c2a441f673  symbols.pes  5 21
1ca563ba453f1302579577ff09e43898c89b6914fe7ff28d9dd4e39ce5a027dd  symbols.pec  5 21
05a79caf4eaac28650ba3e3e8db9e64ebf18d3df1e3df1a4777d9c4ae745b1e0  symbols.jef  33 59
d4c08aa21a81f129109f3c3fbf56edcd72083dc369a054a70d1430be174b6ea7  trim-after.pes  5
4fb93b5e7931dedd01907de38feb0e6f4403aa75fd2aa9204360251ce2d6cf40  trim-after.pec  5
e9facc319b3b2ac109fe165ef49ccea0de647ef84aeda3068d23b921d8b09ecc  trim-after.jef  33
324d7a04e1c395a96e364a2edc62faea9af92cea23f5bb15ec49f18e27d8d5d9  opened-design.pes  5 21
67154131ddd549171f3fc5d475ca7201eb4e0ff7e31e6e64ce9a14ade1486765  opened-design.pec  5 21
fc8c6f8ff1631323cd555418f77d00cfe115736e5c8c3a0175c1d7ebfb413ff6  opened-design.jef  33 12
9036cf5c149bd58c818780f8ea406eb0c75c7a7c6b8e479dbe71d6d7f50ece32  opened-design-clipped.pes  5
cd364ffc2f450c3fb20324c5043905e034558351078bae943992a2f1f8aee72b  opened-design-clipped.pec  5
49860d6b1c22952db3d338e262fc7994b3a069c8cdcdd31ba7e7aa30eb9fa6fe  opened-design-clipped.jef  33
5774353ebd951556d479a1ca527ba108ee316337724e07c96c7c6f31a9879f5b  opened-design-cut-at-block-ends.pes  5 55
41afd61150b4a1c95a64ab19d045d90b91c398cbfc7b7e7417f61d1a222f2b1d  opened-design-cut-at-block-ends.pec  5 55
d94eb984a5171bdb88240ddc94120244f53cef38d50df07ebc523becb9c17127  opened-design-cut-at-block-ends.jef  33 59
30571a7382f6c0f6cea61412004329ef772412f024672a9ef893619677fd72b0  mixed-stitch-style-fill.pes  5 37 21 7 52 24
2b1a1dbcf2b7a1959abd51268cd1cee8f5b84bf2fc30749bc9b982ee4f682e38  mixed-stitch-style-fill.pec  5 37 21 7 52 24
d5e0b5b76165e9597146c7facf20e9b6676af191ce2a26bb7c86d8d4aaa1ccb6  mixed-stitch-style-fill.jef  33 45 59 59 71 14
68404f49e216663779cda4a49cc079fee4a22421755173c3d73428a874324419  mixed-stitch-style-satin.pes  5 37 21 7 52 24
f0575ed808b419cb6322e8d683c76a00d58805f10db411581e07bdb15c7c5c96  mixed-stitch-style-satin.pec  5 37 21 7 52 24
a3a929d81ec9d123764ce19c7068089c96fa17e3da7071048966cb6c7288ce42  mixed-stitch-style-satin.jef  33 45 59 59 71 14
de0d2f4e0cd32f5392dfe444a2a7c3c8b3db8e9d12b2e3cdc2335638fed426a5  mixed-stitch-style-running.pes  5 37 21 7 52 24
3274e196b0a24188b69ebce599cce244e08172bc663e96ae82356dce542b1879  mixed-stitch-style-running.pec  5 37 21 7 52 24
e74b5ee8658196e1174f45bd5b04c4595adcc81e09992a4a1450d6b508de6a16  mixed-stitch-style-running.jef  33 45 59 59 71 14
c38e65e5eb530c8fb470e5baef4fd9e762e84ffe39bb3399dc0e6cf6b013f02f  mixed-every-option.pes  5 37 21 7 52 24
ec17c33f176b4a9da456a6a9f21f63990209cc09e933689433359f0420ee968c  mixed-every-option.pec  5 37 21 7 52 24
e382d7a5b85d40faba1c384b99e425a36268ae2a3d817afa17e5a8bdfb161ac3  mixed-every-option.jef  33 45 59 59 71 14
647016fa5f050c66e1e2adc4792c1f19c14746f651a2f8f9bf63d997d3cac4d5  mixed-shrunk.pes  5 37 21 7 52 24
0116275ca20098bbbd7e80e1bbee1bf294f561da86726d9165049395a593c910  mixed-shrunk.pec  5 37 21 7 52 24
8f233e1387f36b54c8ea167469c09a366cd6696b1db24e49d2594257f074f215  mixed-shrunk.jef  33 45 59 59 71 14
";

/// The PES, PEC and JEF exports of [`ARTWORK`] apart from their thread numbers, listed as
/// [`SYNTHETIC_THREADS`] is.
const ARTWORK_THREADS: &str = "
2e75c5d4920528b816c8bfe3c6d2965032695332d0e87f79d3016f48374c2c15  bullseye-twemoji.pes  29 32 5 33 20 36
8337ee76e87dd3d31bfac3e432aa93804696592921cb0333c075057d575d8b87  bullseye-twemoji.pec  29 32 5 33 20 36
f1d38145b030ab084d0360845649b99fdb6428059e139d7c07d024bbda436f3e  bullseye-twemoji.jef  23 28 33 72 1 11
13b66fbf44372777c53904bb86183f924b459a5fb319149b1d94315a966f4233  fox-twemoji.clipart.pes  52 12 20 51
4769f08394494c5c9c9476e66892725d0c71134daac5748008bd30f88de97e20  fox-twemoji.clipart.pec  52 12 20 51
50fb0d66129a6998f9546b0f2cb96f42a17c57a5c786737526059f1178fa84aa  fox-twemoji.clipart.jef  71 13 1 42
9fe69aa19dfeb91e95248248b955000a73622523ca889c59ac5c3e84130fb380  heart-suit-twemoji.clipart.pes  51
6427722b71f6dc1264ac93d1a41727e8440d7924c5d452958202b29e7fc20117  heart-suit-twemoji.clipart.pec  51
5551a0d48ca3497800a8cdb669094699d1d27b4f08c3a67f5a3d4cb729d7500b  heart-suit-twemoji.clipart.jef  42
759b74dfeebe531ef1d531ac0025547a1711930912cf925c113b0f50fab912a6  joystick-twemoji.pes  16 48 23 39
f5812a8d5555df69ad6a06776680eefc35010bf8246b9670781750df064479aa  joystick-twemoji.pec  16 48 23 39
efdb43ed6fa33d17ca0925ec1c1aecf1b9d5fd5c7ece602067f388b04e30327e  joystick-twemoji.jef  21 33 50 50
17f54ebfd152b2d4b8c57c029bf3a5d3981f2c38b92828966443f5a9013fe9d8  pool-8-ball-twemoji.pes  23 29 23
7d7b839b6e358f2dfbee2166a583c60ac857ceb7d785a52bd1ce82973f1b75fc  pool-8-ball-twemoji.pec  23 29 23
10af81e2aa90c9e6b956d8a3b39932af0f19cd76070f7e02447b2926efbde8f0  pool-8-ball-twemoji.jef  50 23 50
5f3d9d63a7f6ba7851bebc971ae131c0b70679b0325bbc3b04ccfb70880bfe91  puzzle-piece-twemoji.pes  37
01cb0ae4a54d67f012dd057c0adec922d0440ca90668a849f59a4f7851a08cca  puzzle-piece-twemoji.pec  37
7405078c0713095cf336caec247c38fa4156836a998d54fb7179218d90ff4737  puzzle-piece-twemoji.jef  22
d5e02703baa739655a04a069848fe447f89995d1b9f45e19692d59667ce4d3ba  smiling-face-with-smiling-eyes-twemoji.clipart.pes  58 43 23
22b8533cb2e8e75511e5bedac1719866d3f0088c8db5ff92bfd33941fdfb091b  smiling-face-with-smiling-eyes-twemoji.clipart.pec  58 43 23
32538916e92bdcb8382ff923169af1eb64c3007dd39a1a579c8d5b81e1cb97a5  smiling-face-with-smiling-eyes-twemoji.clipart.jef  73 9 1
f30a80819968b3a23868b9c50458e9c0b1db39ae81f61689a61d363376002ae3  sos-button-fluent.clipart.pes  48 29
8fb1ce669f24544b8dcc45634fd23e430fb8e1afc2f684196d678a611706905d  sos-button-fluent.clipart.pec  48 29
e13801f627417dc76930c06d43e842b754ecac75e8725816ba76db7b89774f77  sos-button-fluent.clipart.jef  32 2
b0761a252aabae4db874192df3a33d8f471957310a74b2400818e5563bdc91e0  video-game-twemoji.pes  23 20 39 12 37 32 5 39
83d7f545fd9097dd6c523ca63b7650f1332b83e5f8604b117313d9c6f07e7663  video-game-twemoji.pec  23 20 39 12 37 32 5 39
788f292bb39e4b26fc1bdcee2cffda70461892392e79a3948b12f6d86dd30577  video-game-twemoji.jef  50 1 50 37 22 28 33 24
9540190ec9bb71ea3259ab1c99ce9849b02e6b572a92b1c9bca22728e0734d10  watermelon-twemoji.clipart.pes  54 29 5 20
02c14a192c1762880a15651a3128b06c3b3244b94e9a205c66ae25af06ec1f6a  watermelon-twemoji.clipart.pec  54 29 5 20
8991b02f2eb2b9ad1fbc2a3b00423452c5bdeee092c6930a059a9d683994c8c2  watermelon-twemoji.clipart.jef  6 51 42 1
91e719c2ddb4782284072f2e0b0ec258deed034332a0b7924e3775897eecb715  zebra-twemoji.clipart.pes  29 20 29 39 23 20 39 20
ab11db170971d29e1d43c6e9b5c1ca631d00dc84d68dccd3104c4b1be783d639  zebra-twemoji.clipart.pec  29 20 29 39 23 20 39 20
96946308b260eed6f18dcca23a67dc25b94b6793907b1beb1af5f6c1357366a6  zebra-twemoji.clipart.jef  51 1 51 50 1 1 50 1
";

/// What each export of [`SYNTHETIC`] holds, read back ([`Summary`]): its name, then its fields.
const SYNTHETIC_SUMMARIES: &str = "
fills.dst  blocks 4  needle 360/135/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.67  shortest 0.32  cells 575
fills.tap  blocks 4  needle 360/135/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.67  shortest 0.32  cells 575
fills.t01  blocks 4  needle 360/135/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.67  shortest 0.32  cells 575
fills.exp  blocks 4  needle 360/135/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.67  shortest 0.32  cells 575
fills.pes  blocks 4  needle 360/135/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.36  cells 573
fills.pec  blocks 4  needle 360/135/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.8  longest 4.60  shortest 0.36  cells 573
fills.jef  blocks 4  needle 360/135/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.9  longest 4.60  shortest 0.40  cells 576
fills.vp3  blocks 4  needle 360/135/301/90  jumps 2/1/1/1  trims 1/0/0/0  size 76.0x70.9  longest 4.60  shortest 0.40  cells 576
strokes.dst  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.50  cells 1592
strokes.tap  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.50  cells 1592
strokes.t01  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.50  cells 1592
strokes.exp  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.50  cells 1592
strokes.pes  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.42  cells 1593
strokes.pec  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.3x89.5  longest 4.53  shortest 0.42  cells 1593
strokes.jef  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.2x89.5  longest 4.67  shortest 0.50  cells 1598
strokes.vp3  blocks 6  needle 353/611/265/66/558/390  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 88.2x89.5  longest 4.67  shortest 0.50  cells 1598
compound-path-with-a-hole.dst  blocks 2  needle 1006/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.tap  blocks 2  needle 1006/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.t01  blocks 2  needle 1006/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.exp  blocks 2  needle 1006/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.pes  blocks 2  needle 1006/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.pec  blocks 2  needle 1006/362  jumps 1/4  trims 0/3  size 74.4x28.5  longest 4.60  shortest 0.36  cells 1068
compound-path-with-a-hole.jef  blocks 2  needle 1006/362  jumps 1/4  trims 0/3  size 74.3x28.5  longest 4.67  shortest 0.41  cells 1068
compound-path-with-a-hole.vp3  blocks 2  needle 1006/362  jumps 1/4  trims 0/3  size 74.3x28.5  longest 4.67  shortest 0.41  cells 1068
clip-groups.dst  blocks 5  needle 1167/1166/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1191
clip-groups.tap  blocks 5  needle 1167/1166/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1191
clip-groups.t01  blocks 5  needle 1167/1166/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1191
clip-groups.exp  blocks 5  needle 1167/1166/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1191
clip-groups.pes  blocks 5  needle 1167/1166/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1192
clip-groups.pec  blocks 5  needle 1167/1166/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1192
clip-groups.jef  blocks 5  needle 1167/1166/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1192
clip-groups.vp3  blocks 5  needle 1167/1166/13/190/315  jumps 1/1/1/4/50  trims 0/0/0/0/2  size 36.1x36.1  longest 4.60  shortest 0.36  cells 1192
symbols.dst  blocks 2  needle 661/437  jumps 3/3  trims 2/2  size 51.4x74.3  longest 4.53  shortest 0.42  cells 558
symbols.tap  blocks 2  needle 661/437  jumps 3/3  trims 2/2  size 51.4x74.3  longest 4.53  shortest 0.42  cells 558
symbols.t01  blocks 2  needle 661/437  jumps 3/3  trims 2/2  size 51.4x74.3  longest 4.53  shortest 0.42  cells 558
symbols.exp  blocks 2  needle 661/437  jumps 3/3  trims 2/2  size 51.4x74.3  longest 4.53  shortest 0.42  cells 558
symbols.pes  blocks 2  needle 661/437  jumps 3/3  trims 2/2  size 51.4x74.3  longest 4.53  shortest 0.36  cells 559
symbols.pec  blocks 2  needle 661/437  jumps 3/3  trims 2/2  size 51.4x74.3  longest 4.53  shortest 0.36  cells 559
symbols.jef  blocks 2  needle 661/437  jumps 3/3  trims 2/2  size 51.4x74.3  longest 4.67  shortest 0.42  cells 562
symbols.vp3  blocks 2  needle 661/437  jumps 3/3  trims 2/2  size 51.4x74.3  longest 4.67  shortest 0.42  cells 562
trim-after.dst  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.50  cells 144
trim-after.tap  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.50  cells 144
trim-after.t01  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.50  cells 144
trim-after.exp  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.50  cells 144
trim-after.pes  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.50  cells 144
trim-after.pec  blocks 1  needle 242  jumps 4  trims 2  size 26.7x5.6  longest 4.53  shortest 0.50  cells 144
trim-after.jef  blocks 1  needle 242  jumps 4  trims 2  size 26.8x5.6  longest 4.53  shortest 0.50  cells 144
trim-after.vp3  blocks 1  needle 242  jumps 4  trims 2  size 26.8x5.6  longest 4.53  shortest 0.50  cells 144
opened-design.dst  blocks 2  needle 360/135  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.tap  blocks 2  needle 360/135  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.t01  blocks 2  needle 360/135  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.exp  blocks 2  needle 360/135  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.pes  blocks 2  needle 360/135  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.pec  blocks 2  needle 360/135  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.jef  blocks 2  needle 360/135  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
opened-design.vp3  blocks 2  needle 360/135  jumps 2/1  trims 1/0  size 35.5x42.6  longest 4.67  shortest 0.36  cells 364
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
mixed-stitch-style-fill.dst  blocks 6  needle 393/331/337/102/546/410  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1443
mixed-stitch-style-fill.tap  blocks 6  needle 393/331/337/102/546/410  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1443
mixed-stitch-style-fill.t01  blocks 6  needle 393/331/337/102/546/410  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1443
mixed-stitch-style-fill.exp  blocks 6  needle 393/331/337/102/546/410  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1443
mixed-stitch-style-fill.pes  blocks 6  needle 393/331/337/102/546/410  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1444
mixed-stitch-style-fill.pec  blocks 6  needle 393/331/337/102/546/410  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1444
mixed-stitch-style-fill.jef  blocks 6  needle 393/331/337/102/546/410  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1440
mixed-stitch-style-fill.vp3  blocks 6  needle 393/331/337/102/546/410  jumps 1/1/1/2/1/1  trims 0/0/0/1/0/0  size 86.0x78.6  longest 4.67  shortest 0.36  cells 1440
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
mixed-shrunk.dst  blocks 6  needle 375/298/368/37/518/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.41  cells 1358
mixed-shrunk.tap  blocks 6  needle 375/298/368/37/518/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.41  cells 1358
mixed-shrunk.t01  blocks 6  needle 375/298/368/37/518/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.41  cells 1358
mixed-shrunk.exp  blocks 6  needle 375/298/368/37/518/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.41  cells 1358
mixed-shrunk.pes  blocks 6  needle 375/298/368/37/518/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.41  cells 1357
mixed-shrunk.pec  blocks 6  needle 375/298/368/37/518/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.46  shortest 0.41  cells 1357
mixed-shrunk.jef  blocks 6  needle 375/298/368/37/518/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.40  cells 1358
mixed-shrunk.vp3  blocks 6  needle 375/298/368/37/518/390  jumps 1/1/1/1/1/1  trims 0/0/0/0/0/0  size 85.6x78.0  longest 4.38  shortest 0.40  cells 1358
";

/// What each export of [`ARTWORK`] holds, read back, listed as [`SYNTHETIC_SUMMARIES`] is.
const ARTWORK_SUMMARIES: &str = "
bullseye-twemoji.dst  blocks 6  needle 1563/998/550/232/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1710
bullseye-twemoji.tap  blocks 6  needle 1563/998/550/232/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1710
bullseye-twemoji.t01  blocks 6  needle 1563/998/550/232/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1710
bullseye-twemoji.exp  blocks 6  needle 1563/998/550/232/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.46  shortest 0.32  cells 1710
bullseye-twemoji.pes  blocks 6  needle 1563/998/550/232/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.50  shortest 0.32  cells 1711
bullseye-twemoji.pec  blocks 6  needle 1563/998/550/232/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.3x45.2  longest 8.50  shortest 0.32  cells 1711
bullseye-twemoji.jef  blocks 6  needle 1563/998/550/232/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.2x45.2  longest 8.50  shortest 0.32  cells 1711
bullseye-twemoji.vp3  blocks 6  needle 1563/998/550/232/290/161  jumps 1/1/1/1/2/1  trims 0/0/0/0/0/0  size 45.2x45.2  longest 8.50  shortest 0.32  cells 1711
fox-twemoji.clipart.dst  blocks 4  needle 1414/460/148/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.04  shortest 0.32  cells 1329
fox-twemoji.clipart.tap  blocks 4  needle 1414/460/148/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.04  shortest 0.32  cells 1329
fox-twemoji.clipart.t01  blocks 4  needle 1414/460/148/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.04  shortest 0.32  cells 1329
fox-twemoji.clipart.exp  blocks 4  needle 1414/460/148/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.04  shortest 0.32  cells 1329
fox-twemoji.clipart.pes  blocks 4  needle 1414/460/148/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.06  shortest 0.32  cells 1328
fox-twemoji.clipart.pec  blocks 4  needle 1414/460/148/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.3x40.7  longest 10.06  shortest 0.32  cells 1328
fox-twemoji.clipart.jef  blocks 4  needle 1414/460/148/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.32  cells 1332
fox-twemoji.clipart.vp3  blocks 4  needle 1414/460/148/248  jumps 1/2/3/2  trims 0/0/2/1  size 45.2x40.7  longest 10.04  shortest 0.32  cells 1332
heart-suit-twemoji.clipart.dst  blocks 1  needle 1374  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1366
heart-suit-twemoji.clipart.tap  blocks 1  needle 1374  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1366
heart-suit-twemoji.clipart.t01  blocks 1  needle 1374  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1366
heart-suit-twemoji.clipart.exp  blocks 1  needle 1374  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1366
heart-suit-twemoji.clipart.pes  blocks 1  needle 1374  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1368
heart-suit-twemoji.clipart.pec  blocks 1  needle 1374  jumps 1  trims 0  size 45.3x42.3  longest 4.61  shortest 0.32  cells 1368
heart-suit-twemoji.clipart.jef  blocks 1  needle 1374  jumps 1  trims 0  size 45.2x42.3  longest 4.55  shortest 0.32  cells 1367
heart-suit-twemoji.clipart.vp3  blocks 1  needle 1374  jumps 1  trims 0  size 45.2x42.3  longest 4.55  shortest 0.32  cells 1367
joystick-twemoji.dst  blocks 4  needle 155/254/1031/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.36  cells 1007
joystick-twemoji.tap  blocks 4  needle 155/254/1031/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.36  cells 1007
joystick-twemoji.t01  blocks 4  needle 155/254/1031/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.36  cells 1007
joystick-twemoji.exp  blocks 4  needle 155/254/1031/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.36  cells 1007
joystick-twemoji.pes  blocks 4  needle 155/254/1031/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.41  cells 1006
joystick-twemoji.pec  blocks 4  needle 155/254/1031/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.4  longest 5.70  shortest 0.41  cells 1006
joystick-twemoji.jef  blocks 4  needle 155/254/1031/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.3  longest 5.70  shortest 0.36  cells 986
joystick-twemoji.vp3  blocks 4  needle 155/254/1031/229  jumps 1/2/4/1  trims 0/1/2/0  size 42.7x45.3  longest 5.70  shortest 0.36  cells 986
pool-8-ball-twemoji.dst  blocks 3  needle 1640/483/314  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.67  shortest 0.32  cells 1757
pool-8-ball-twemoji.tap  blocks 3  needle 1640/483/314  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.67  shortest 0.32  cells 1757
pool-8-ball-twemoji.t01  blocks 3  needle 1640/483/314  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.67  shortest 0.32  cells 1757
pool-8-ball-twemoji.exp  blocks 3  needle 1640/483/314  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.67  shortest 0.32  cells 1757
pool-8-ball-twemoji.pes  blocks 3  needle 1640/483/314  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.60  shortest 0.32  cells 1758
pool-8-ball-twemoji.pec  blocks 3  needle 1640/483/314  jumps 1/1/1  trims 0/0/0  size 46.3x46.3  longest 4.60  shortest 0.32  cells 1758
pool-8-ball-twemoji.jef  blocks 3  needle 1640/483/314  jumps 1/1/1  trims 0/0/0  size 46.2x46.2  longest 4.60  shortest 0.32  cells 1756
pool-8-ball-twemoji.vp3  blocks 3  needle 1640/483/314  jumps 1/1/1  trims 0/0/0  size 46.2x46.2  longest 4.60  shortest 0.32  cells 1756
puzzle-piece-twemoji.dst  blocks 1  needle 1325  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.36  cells 1203
puzzle-piece-twemoji.tap  blocks 1  needle 1325  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.36  cells 1203
puzzle-piece-twemoji.t01  blocks 1  needle 1325  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.36  cells 1203
puzzle-piece-twemoji.exp  blocks 1  needle 1325  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.36  cells 1203
puzzle-piece-twemoji.pes  blocks 1  needle 1325  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.36  cells 1200
puzzle-piece-twemoji.pec  blocks 1  needle 1325  jumps 1  trims 0  size 45.3x45.2  longest 4.67  shortest 0.36  cells 1200
puzzle-piece-twemoji.jef  blocks 1  needle 1325  jumps 1  trims 0  size 45.2x45.2  longest 4.60  shortest 0.41  cells 1204
puzzle-piece-twemoji.vp3  blocks 1  needle 1325  jumps 1  trims 0  size 45.2x45.2  longest 4.60  shortest 0.41  cells 1204
smiling-face-with-smiling-eyes-twemoji.clipart.dst  blocks 3  needle 1907/373/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1853
smiling-face-with-smiling-eyes-twemoji.clipart.tap  blocks 3  needle 1907/373/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1853
smiling-face-with-smiling-eyes-twemoji.clipart.t01  blocks 3  needle 1907/373/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1853
smiling-face-with-smiling-eyes-twemoji.clipart.exp  blocks 3  needle 1907/373/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1853
smiling-face-with-smiling-eyes-twemoji.clipart.pes  blocks 3  needle 1907/373/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1852
smiling-face-with-smiling-eyes-twemoji.clipart.pec  blocks 3  needle 1907/373/388  jumps 1/2/3  trims 0/1/0  size 47.8x47.7  longest 5.92  shortest 0.32  cells 1852
smiling-face-with-smiling-eyes-twemoji.clipart.jef  blocks 3  needle 1907/373/388  jumps 1/2/3  trims 0/1/0  size 47.7x47.7  longest 5.92  shortest 0.32  cells 1853
smiling-face-with-smiling-eyes-twemoji.clipart.vp3  blocks 3  needle 1907/373/388  jumps 1/2/3  trims 0/1/0  size 47.7x47.7  longest 5.92  shortest 0.32  cells 1853
sos-button-fluent.clipart.dst  blocks 2  needle 1988/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.67  shortest 0.41  cells 2076
sos-button-fluent.clipart.tap  blocks 2  needle 1988/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.67  shortest 0.41  cells 2076
sos-button-fluent.clipart.t01  blocks 2  needle 1988/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.67  shortest 0.41  cells 2076
sos-button-fluent.clipart.exp  blocks 2  needle 1988/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.67  shortest 0.41  cells 2076
sos-button-fluent.clipart.pes  blocks 2  needle 1988/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.40  cells 2076
sos-button-fluent.clipart.pec  blocks 2  needle 1988/770  jumps 1/3  trims 0/1  size 45.3x45.3  longest 4.60  shortest 0.40  cells 2076
sos-button-fluent.clipart.jef  blocks 2  needle 1988/770  jumps 1/3  trims 0/1  size 45.2x45.2  longest 4.60  shortest 0.32  cells 2075
sos-button-fluent.clipart.vp3  blocks 2  needle 1988/770  jumps 1/3  trims 0/1  size 45.2x45.2  longest 4.60  shortest 0.32  cells 2075
video-game-twemoji.dst  blocks 8  needle 581/1199/142/33/32/32/33/140  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.95  shortest 0.32  cells 1222
video-game-twemoji.tap  blocks 8  needle 581/1199/142/33/32/32/33/140  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.95  shortest 0.32  cells 1222
video-game-twemoji.t01  blocks 8  needle 581/1199/142/33/32/32/33/140  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.95  shortest 0.32  cells 1222
video-game-twemoji.exp  blocks 8  needle 581/1199/142/33/32/32/33/140  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.95  shortest 0.32  cells 1222
video-game-twemoji.pes  blocks 8  needle 581/1199/142/33/32/32/33/140  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.88  shortest 0.32  cells 1221
video-game-twemoji.pec  blocks 8  needle 581/1199/142/33/32/32/33/140  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.88  shortest 0.32  cells 1221
video-game-twemoji.jef  blocks 8  needle 581/1199/142/33/32/32/33/140  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.88  shortest 0.32  cells 1217
video-game-twemoji.vp3  blocks 8  needle 581/1199/142/33/32/32/33/140  jumps 4/3/2/1/1/1/1/4  trims 2/2/0/0/0/0/0/0  size 45.2x32.1  longest 4.88  shortest 0.32  cells 1217
watermelon-twemoji.clipart.dst  blocks 4  needle 411/367/605/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.36  cells 966
watermelon-twemoji.clipart.tap  blocks 4  needle 411/367/605/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.36  cells 966
watermelon-twemoji.clipart.t01  blocks 4  needle 411/367/605/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.36  cells 966
watermelon-twemoji.clipart.exp  blocks 4  needle 411/367/605/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.36  cells 966
watermelon-twemoji.clipart.pes  blocks 4  needle 411/367/605/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.36  cells 966
watermelon-twemoji.clipart.pec  blocks 4  needle 411/367/605/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.67  shortest 0.36  cells 966
watermelon-twemoji.clipart.jef  blocks 4  needle 411/367/605/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.74  shortest 0.40  cells 966
watermelon-twemoji.clipart.vp3  blocks 4  needle 411/367/605/152  jumps 1/1/1/8  trims 0/0/0/0  size 46.2x32.1  longest 4.74  shortest 0.40  cells 966
zebra-twemoji.clipart.dst  blocks 8  needle 1604/322/270/42/25/563/46/768  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
zebra-twemoji.clipart.tap  blocks 8  needle 1604/322/270/42/25/563/46/768  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
zebra-twemoji.clipart.t01  blocks 8  needle 1604/322/270/42/25/563/46/768  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
zebra-twemoji.clipart.exp  blocks 8  needle 1604/322/270/42/25/563/46/768  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
zebra-twemoji.clipart.pes  blocks 8  needle 1604/322/270/42/25/563/46/768  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1569
zebra-twemoji.clipart.pec  blocks 8  needle 1604/322/270/42/25/563/46/768  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1569
zebra-twemoji.clipart.jef  blocks 8  needle 1604/322/270/42/25/563/46/768  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
zebra-twemoji.clipart.vp3  blocks 8  needle 1604/322/270/42/25/563/46/768  jumps 2/2/2/1/1/3/1/9  trims 1/0/1/0/0/1/0/3  size 45.3x45.9  longest 6.98  shortest 0.32  cells 1570
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
    // Its angle, 150° counter-clockwise on the page, is the planner's 30° the listing was recorded
    // with (the commands took the planner's angle until 2026-10-24).
    let every_option = json!({
        "density": 0.5, "stitchLength": 3, "stitchAngle": 150, "pullCompensation": [0.3, 0.1], "pullCompensationPercent": 10,
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
    let pattern = Pattern { name: "cuts".into(), blocks: vec![a, b], ..Pattern::default() };
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
