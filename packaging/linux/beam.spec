Name:           beam
Version:        0.1.1
Release:        1%{?dist}
Summary:        Professional, non-destructive RAW photo editor and library manager
License:        GPL-3.0-only
URL:            https://mozcelik14.github.io/beaM/
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  rust >= 1.75
BuildRequires:  cargo

Requires:       hicolor-icon-theme
Requires:       shared-mime-info

%description
beaM. is an independent, high-performance desktop photographic darkroom
and asset manager built in 100% pure Rust, powered by wgpu and egui.
It features a 32-bit floating point non-destructive GPU pipeline,
RawTherapee-style in-app directory navigation, tone curve and color controls,
and batch export capabilities.

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}%{_bindir}
mkdir -p %{buildroot}%{_datadir}/applications
mkdir -p %{buildroot}%{_datadir}/metainfo
mkdir -p %{buildroot}%{_datadir}/mime/packages

install -m 0755 target/release/beam %{buildroot}%{_bindir}/beam
install -m 0644 packaging/beam.desktop %{buildroot}%{_datadir}/applications/beam.desktop
install -m 0644 packaging/com.github.mozcelik14.beam.metainfo.xml %{buildroot}%{_datadir}/metainfo/com.github.mozcelik14.beam.metainfo.xml
install -m 0644 packaging/beam-mime.xml %{buildroot}%{_datadir}/mime/packages/beam-mime.xml

for s in 16 32 48 64 128 256 512; do
    if [ -f packaging/icons/hicolor/${s}x${s}/apps/beam.png ]; then
        mkdir -p %{buildroot}%{_datadir}/icons/hicolor/${s}x${s}/apps
        install -m 0644 packaging/icons/hicolor/${s}x${s}/apps/beam.png %{buildroot}%{_datadir}/icons/hicolor/${s}x${s}/apps/beam.png
    fi
done

%post
if [ -x %{_bindir}/update-desktop-database ]; then
    %{_bindir}/update-desktop-database -q %{_datadir}/applications || true
fi
if [ -x %{_bindir}/update-mime-database ]; then
    %{_bindir}/update-mime-database %{_datadir}/mime || true
fi
if [ -x %{_bindir}/gtk-update-icon-cache ]; then
    %{_bindir}/gtk-update-icon-cache -q -t -f %{_datadir}/icons/hicolor || true
fi

%postun
if [ $1 -eq 0 ]; then
    if [ -x %{_bindir}/update-desktop-database ]; then
        %{_bindir}/update-desktop-database -q %{_datadir}/applications || true
    fi
    if [ -x %{_bindir}/update-mime-database ]; then
        %{_bindir}/update-mime-database %{_datadir}/mime || true
    fi
    if [ -x %{_bindir}/gtk-update-icon-cache ]; then
        %{_bindir}/gtk-update-icon-cache -q -t -f %{_datadir}/icons/hicolor || true
    fi
fi

%files
%{_bindir}/beam
%{_datadir}/applications/beam.desktop
%{_datadir}/metainfo/com.github.mozcelik14.beam.metainfo.xml
%{_datadir}/mime/packages/beam-mime.xml
%{_datadir}/icons/hicolor/*/apps/beam.png

%changelog
* Fri Nov 06 2026 beaM. Contributors <https://github.com/MOzcelik14/beaM> - 0.1.1-1
- RawTherapee-style file browser, .beam extension standardization, unsaved changes dialog.
* Wed Oct 28 2026 beaM. Contributors <https://github.com/MOzcelik14/beaM> - 0.1.0-1
- Initial release.
