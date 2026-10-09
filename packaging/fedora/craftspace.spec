# Fedora / Copr package for CraftSpace. The source tarball includes vendored crates
# (`cargo vendor`), so it builds without network access; .copr/Makefile makes it.

%global debug_package %{nil}

Name:           craftspace
Version:        0.1.0
Release:        1%{?dist}
Summary:        Installer and update manager for the open-source ArtCraft creative apps
License:        MIT OR Apache-2.0
URL:            https://github.com/emircesur/craftspace
Source0:        %{name}-%{version}-vendored.tar.gz

BuildRequires:  cargo >= 1.85
BuildRequires:  rust >= 1.85
BuildRequires:  gcc
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib

# The window uses OpenGL through winit (X11 or Wayland), loaded at run time.
# The window system libraries are loaded at run time, so rpm doesn't find them by itself.
Requires:       libglvnd-glx
Requires:       libglvnd-egl
Requires:       libxkbcommon
Requires:       libxkbcommon-x11
Requires:       libX11
Requires:       libX11-xcb
Requires:       libXcursor
Requires:       libXrandr
Requires:       libXi
Requires:       libwayland-client
Recommends:     polkit
Recommends:     fontconfig

%description
CraftSpace installs and updates the open-source ArtCraft apps (PhotoCraft, LightCraft,
VectorCraft, FilmCraft, GridCraft and more): checksum-verified downloads, update channels and
rollback, recent files, news and tutorials. Installed from this package, CraftSpace is updated
by dnf; it can also install the ArtCraft apps themselves as RPMs.

%prep
%autosetup -n %{name}-%{version}

%build
cargo build --release --offline --locked -p craftspace -p craftspace-cli

%install
install -Dm755 target/release/craftspace %{buildroot}%{_bindir}/craftspace
install -Dm755 target/release/craftspace-cli %{buildroot}%{_bindir}/craftspace-cli
install -Dm644 packaging/linux/craftspace.desktop %{buildroot}%{_datadir}/applications/craftspace.desktop
install -Dm644 assets/craftspace-256.png %{buildroot}%{_datadir}/icons/hicolor/256x256/apps/craftspace.png
install -Dm644 packaging/linux/io.github.emircesur.craftspace.metainfo.xml %{buildroot}%{_metainfodir}/io.github.emircesur.craftspace.metainfo.xml

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/craftspace.desktop
appstream-util validate-relax --nonet %{buildroot}%{_metainfodir}/io.github.emircesur.craftspace.metainfo.xml
cargo test --release --offline --locked -p craftspace-core

%files
%license LICENSE-MIT LICENSE-APACHE
%doc README.md
%{_bindir}/craftspace
%{_bindir}/craftspace-cli
%{_datadir}/applications/craftspace.desktop
%{_datadir}/icons/hicolor/256x256/apps/craftspace.png
%{_metainfodir}/io.github.emircesur.craftspace.metainfo.xml

%changelog
* Fri Oct 09 2026 CraftSpace contributors - 0.1.0-1
- First release
