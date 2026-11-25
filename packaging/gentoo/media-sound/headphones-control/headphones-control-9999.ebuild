# Copyright 2026 Al-Hadrusi
# Distributed under the terms of the MIT License

EAPI=8

RUST_MIN_VER="1.95.0"

inherit cargo desktop git-r3 xdg

DESCRIPTION="Unofficial control app for Sony Bluetooth headphones (ANC, ambient sound, EQ)"
HOMEPAGE="https://github.com/Al-Hadrusi/SonyGUIAudioDrivers"
EGIT_REPO_URI="https://github.com/Al-Hadrusi/SonyGUIAudioDrivers.git"
EGIT_BRANCH="main"

LICENSE="MIT"
# Dependent crate licenses
LICENSE+=" Apache-2.0 BSD-2 ISC MIT OFL-1.1 Unicode-3.0 UbuntuFontLicense-1.0 Unlicense ZLIB"
SLOT="0"
IUSE="+bluetooth +notifications +tray"

DEPEND="bluetooth? ( sys-apps/dbus )"
# The window library loads X11/Wayland/GL libraries at runtime.
RDEPEND="
	${DEPEND}
	bluetooth? ( net-wireless/bluez )
	media-libs/libglvnd
	x11-libs/libxkbcommon[X]
	|| (
		dev-libs/wayland
		(
			x11-libs/libX11
			x11-libs/libXcursor
			x11-libs/libXi
			x11-libs/libXrandr
		)
	)
"
BDEPEND="virtual/pkgconfig"

src_unpack() {
	git-r3_src_unpack
	cargo_live_src_unpack
}

src_configure() {
	local myfeatures=(
		$(usev bluetooth)
		$(usev notifications)
		$(usev tray)
	)
	cargo_src_configure --no-default-features
}

src_install() {
	cargo_src_install

	domenu packaging/linux/headphones-control.desktop
	newicon -s scalable assets/headphones-control.svg headphones-control.svg
	newicon -s 256 assets/headphones-control-256.png headphones-control.png
	einstalldocs
}

pkg_postinst() {
	xdg_pkg_postinst

	if use bluetooth; then
		elog "Pair your headset with BlueZ (e.g. bluetoothctl) before starting the app."
		elog "The kernel needs CONFIG_BT and CONFIG_BT_RFCOMM."
	fi
}
