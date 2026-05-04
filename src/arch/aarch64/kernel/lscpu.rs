//! Helpers for mapping the implementer and part number fields in the MIDR_EL1
//! register to a pretty, human-readable CPU core name.
//!
//! The lookup tables here are taken from util-linux:
//! <https://github.com/util-linux/util-linux/blob/da322604a45f8094d439ea64be7d44cfea1f500a/sys-utils/lscpu-arm.c>
//!
//! util-linux is licensed under the GPL, but the implementer/model name tables
//! are uncopyrightable and explicitly intended to be reused in other projects,
//! see for example this commit:
//! <https://github.com/util-linux/util-linux/commit/16724407c858f4d3e2d2497f758b678b644c0011>

type PartsTable = &'static [(u16, &'static str)];

static ARM_PARTS: PartsTable = &[
	(0x810, "ARM810"),
	(0x920, "ARM920"),
	(0x922, "ARM922"),
	(0x926, "ARM926"),
	(0x940, "ARM940"),
	(0x946, "ARM946"),
	(0x966, "ARM966"),
	(0xa20, "ARM1020"),
	(0xa22, "ARM1022"),
	(0xa26, "ARM1026"),
	(0xb02, "ARM11-MPCore"),
	(0xb36, "ARM1136"),
	(0xb56, "ARM1156"),
	(0xb76, "ARM1176"),
	(0xc05, "Cortex-A5"),
	(0xc07, "Cortex-A7"),
	(0xc08, "Cortex-A8"),
	(0xc09, "Cortex-A9"),
	(0xc0d, "Cortex-A17"),
	(0xc0f, "Cortex-A15"),
	(0xc0e, "Cortex-A17"),
	(0xc14, "Cortex-R4"),
	(0xc15, "Cortex-R5"),
	(0xc17, "Cortex-R7"),
	(0xc18, "Cortex-R8"),
	(0xc20, "Cortex-M0"),
	(0xc21, "Cortex-M1"),
	(0xc23, "Cortex-M3"),
	(0xc24, "Cortex-M4"),
	(0xc27, "Cortex-M7"),
	(0xc60, "Cortex-M0+"),
	(0xd01, "Cortex-A32"),
	(0xd02, "Cortex-A34"),
	(0xd03, "Cortex-A53"),
	(0xd04, "Cortex-A35"),
	(0xd05, "Cortex-A55"),
	(0xd06, "Cortex-A65"),
	(0xd07, "Cortex-A57"),
	(0xd08, "Cortex-A72"),
	(0xd09, "Cortex-A73"),
	(0xd0a, "Cortex-A75"),
	(0xd0b, "Cortex-A76"),
	(0xd0c, "Neoverse-N1"),
	(0xd0d, "Cortex-A77"),
	(0xd0e, "Cortex-A76AE"),
	(0xd13, "Cortex-R52"),
	(0xd14, "Cortex-R82AE"),
	(0xd15, "Cortex-R82"),
	(0xd16, "Cortex-R52+"),
	(0xd20, "Cortex-M23"),
	(0xd21, "Cortex-M33"),
	(0xd24, "Cortex-M52"),
	(0xd22, "Cortex-M55"),
	(0xd23, "Cortex-M85"),
	(0xd40, "Neoverse-V1"),
	(0xd41, "Cortex-A78"),
	(0xd42, "Cortex-A78AE"),
	(0xd43, "Cortex-A65AE"),
	(0xd44, "Cortex-X1"),
	(0xd46, "Cortex-A510"),
	(0xd47, "Cortex-A710"),
	(0xd48, "Cortex-X2"),
	(0xd49, "Neoverse-N2"),
	(0xd4a, "Neoverse-E1"),
	(0xd4b, "Cortex-A78C"),
	(0xd4c, "Cortex-X1C"),
	(0xd4d, "Cortex-A715"),
	(0xd4e, "Cortex-X3"),
	(0xd4f, "Neoverse-V2"),
	(0xd80, "Cortex-A520"),
	(0xd81, "Cortex-A720"),
	(0xd82, "Cortex-X4"),
	(0xd83, "Neoverse-V3AE"),
	(0xd84, "Neoverse-V3"),
	(0xd85, "Cortex-X925"),
	(0xd87, "Cortex-A725"),
	(0xd88, "Cortex-A520AE"),
	(0xd89, "Cortex-A720AE"),
	(0xd8a, "C1-Nano"),
	(0xd8b, "C1-Pro"),
	(0xd8c, "C1-Ultra"),
	(0xd8e, "Neoverse-N3"),
	(0xd8f, "Cortex-A320"),
	(0xd90, "C1-Premium"),
];

static BRCM_PARTS: PartsTable = &[
	(0x0f, "Brahma-B15"),
	(0x100, "Brahma-B53"),
	(0x516, "ThunderX2"),
];

static DEC_PARTS: PartsTable = &[(0xa10, "SA110"), (0xa11, "SA1100")];

static CAVIUM_PARTS: PartsTable = &[
	(0x0a0, "ThunderX"),
	(0x0a1, "ThunderX-88XX"),
	(0x0a2, "ThunderX-81XX"),
	(0x0a3, "ThunderX-83XX"),
	(0x0af, "ThunderX2-99xx"),
	(0x0b0, "OcteonTX2"),
	(0x0b1, "OcteonTX2-98XX"),
	(0x0b2, "OcteonTX2-96XX"),
	(0x0b3, "OcteonTX2-95XX"),
	(0x0b4, "OcteonTX2-95XXN"),
	(0x0b5, "OcteonTX2-95XXMM"),
	(0x0b6, "OcteonTX2-95XXO"),
	(0x0b8, "ThunderX3-T110"),
];

static APM_PARTS: PartsTable = &[(0x000, "X-Gene")];

static QCOM_PARTS: PartsTable = &[
	(0x001, "Oryon"),
	(0x00f, "Scorpion"),
	(0x02d, "Scorpion"),
	(0x04d, "Krait"),
	(0x06f, "Krait"),
	(0x201, "Kryo"),
	(0x205, "Kryo"),
	(0x211, "Kryo"),
	(0x800, "Falkor-V1/Kryo"),
	(0x801, "Kryo-V2"),
	(0x802, "Kryo-3XX-Gold"),
	(0x803, "Kryo-3XX-Silver"),
	(0x804, "Kryo-4XX-Gold"),
	(0x805, "Kryo-4XX-Silver"),
	(0xc00, "Falkor"),
	(0xc01, "Saphira"),
];

static SAMSUNG_PARTS: PartsTable = &[
	(0x001, "exynos-m1"),
	(0x002, "exynos-m3"),
	(0x003, "exynos-m4"),
	(0x004, "exynos-m5"),
];

static NVIDIA_PARTS: PartsTable = &[
	(0x000, "Denver"),
	(0x003, "Denver-2"),
	(0x004, "Carmel"),
	(0x010, "Olympus"),
];

static MARVELL_PARTS: PartsTable = &[
	(0x131, "Feroceon-88FR131"),
	(0x581, "PJ4/PJ4b"),
	(0x584, "PJ4B-MP"),
];

static APPLE_PARTS: PartsTable = &[
	(0x000, "Swift"),
	(0x001, "Cyclone"),
	(0x002, "Typhoon"),
	(0x003, "Typhoon/Capri"),
	(0x004, "Twister"),
	(0x005, "Twister/Elba/Malta"),
	(0x006, "Hurricane"),
	(0x007, "Hurricane/Myst"),
	(0x008, "Monsoon"),
	(0x009, "Mistral"),
	(0x00b, "Vortex"),
	(0x00c, "Tempest"),
	(0x00f, "Tempest-M9"),
	(0x010, "Vortex/Aruba"),
	(0x011, "Tempest/Aruba"),
	(0x012, "Lightning"),
	(0x013, "Thunder"),
	(0x020, "Icestorm-A14"),
	(0x021, "Firestorm-A14"),
	(0x022, "Icestorm-M1"),
	(0x023, "Firestorm-M1"),
	(0x024, "Icestorm-M1-Pro"),
	(0x025, "Firestorm-M1-Pro"),
	(0x026, "Thunder-M10"),
	(0x028, "Icestorm-M1-Max"),
	(0x029, "Firestorm-M1-Max"),
	(0x030, "Blizzard-A15"),
	(0x031, "Avalanche-A15"),
	(0x032, "Blizzard-M2"),
	(0x033, "Avalanche-M2"),
	(0x034, "Blizzard-M2-Pro"),
	(0x035, "Avalanche-M2-Pro"),
	(0x036, "Sawtooth-A16"),
	(0x037, "Everest-A16"),
	(0x038, "Blizzard-M2-Max"),
	(0x039, "Avalanche-M2-Max"),
];

static FARADAY_PARTS: PartsTable = &[(0x526, "FA526"), (0x626, "FA626")];

static INTEL_PARTS: PartsTable = &[
	(0x200, "i80200"),
	(0x210, "PXA250A"),
	(0x212, "PXA210A"),
	(0x242, "i80321-400"),
	(0x243, "i80321-600"),
	(0x290, "PXA250B/PXA26x"),
	(0x292, "PXA210B"),
	(0x2c2, "i80321-400-B0"),
	(0x2c3, "i80321-600-B0"),
	(0x2d0, "PXA250C/PXA255/PXA26x"),
	(0x2d2, "PXA210C"),
	(0x411, "PXA27x"),
	(0x41c, "IPX425-533"),
	(0x41d, "IPX425-400"),
	(0x41f, "IPX425-266"),
	(0x682, "PXA32x"),
	(0x683, "PXA930/PXA935"),
	(0x688, "PXA30x"),
	(0x689, "PXA31x"),
	(0xb11, "SA1110"),
	(0xc12, "IPX1200"),
];

static FUJITSU_PARTS: PartsTable = &[(0x001, "A64FX"), (0x003, "MONAKA")];

static HISI_PARTS: PartsTable = &[
	(0xd01, "TaiShan-v110"),
	(0xd02, "TaiShan-v120"),
	(0xd40, "Cortex-A76"),
	(0xd41, "Cortex-A77"),
];

static AMPERE_PARTS: PartsTable = &[(0xac3, "Ampere-1"), (0xac4, "Ampere-1a")];

static FT_PARTS: PartsTable = &[
	(0x303, "FTC310"),
	(0x660, "FTC660"),
	(0x661, "FTC661"),
	(0x662, "FTC662"),
	(0x663, "FTC663"),
	(0x664, "FTC664"),
	(0x862, "FTC862"),
];

static MS_PARTS: PartsTable = &[(0xd49, "Azure-Cobalt-100")];

static IMPLEMENTERS: &[(u8, &str, PartsTable)] = &[
	(0x41, "ARM", ARM_PARTS),
	(0x42, "Broadcom", BRCM_PARTS),
	(0x43, "Cavium", CAVIUM_PARTS),
	(0x44, "DEC", DEC_PARTS),
	(0x46, "FUJITSU", FUJITSU_PARTS),
	(0x48, "HiSilicon", HISI_PARTS),
	(0x49, "Infineon", &[]),           // no parts known
	(0x4d, "Motorola/Freescale", &[]), // no parts known
	(0x4e, "NVIDIA", NVIDIA_PARTS),
	(0x50, "APM", APM_PARTS),
	(0x51, "Qualcomm", QCOM_PARTS),
	(0x53, "Samsung", SAMSUNG_PARTS),
	(0x56, "Marvell", MARVELL_PARTS),
	(0x61, "Apple", APPLE_PARTS),
	(0x66, "Faraday", FARADAY_PARTS),
	(0x69, "Intel", INTEL_PARTS),
	(0x6d, "Microsoft", MS_PARTS),
	(0x70, "Phytium", FT_PARTS),
	(0xc0, "Ampere", AMPERE_PARTS),
];

pub fn pretty_implementer_and_part(
	implementer_raw: u8,
	part_raw: u16,
) -> Option<(&'static str, Option<&'static str>)> {
	let (implementer, parts) = IMPLEMENTERS
		.iter()
		.copied()
		.find_map(|(id, name, parts)| (id == implementer_raw).then_some((name, parts)))?;

	let part = parts
		.iter()
		.copied()
		.find_map(|(id, name)| (id == part_raw).then_some(name));

	Some((implementer, part))
}
