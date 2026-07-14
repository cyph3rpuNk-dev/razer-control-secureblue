# CI-oriented spec: builds with network access for crates.io.  A COPR/offline
# build needs vendored sources (rust2rpm or cargo vendor); tracked for the
# packaging milestone.

Name:           razer-control-secureblue
Version:        0.1.0
Release:        1%{?dist}
Summary:        Safety-first Razer Blade control for Atomic Linux
License:        GPL-2.0-only
URL:            https://github.com/cyph3rpuNk-dev/razer-control-secureblue
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  systemd-rpm-macros
# ksni (StatusNotifierItem tray) links libdbus
BuildRequires:  dbus-devel

%description
Per-user, socket-activated control daemon for Razer Blade laptops, built for
Fedora Atomic and Secureblue. Session-scoped uaccess HID permissions instead
of world-writable device nodes, a verified per-model capability table, and an
automatic-fan failsafe. The current release contains the policy layer and a
dry-run daemon; it sends no hardware commands.

%prep
%autosetup

%build
cargo build --release --locked -p razer-control-secureblue -p razer-control-gui -p razer-control-tray

%check
cargo test --release --locked -p razer-control-secureblue -p razer-control-gui -p razer-control-tray

%install
install -Dm0755 target/release/razer-control %{buildroot}%{_bindir}/razer-control
install -Dm0755 target/release/razer-control-gui %{buildroot}%{_bindir}/razer-control-gui
install -Dm0755 target/release/razer-control-tray %{buildroot}%{_bindir}/razer-control-tray
install -Dm0644 packaging/razer-control-gui.desktop %{buildroot}%{_datadir}/applications/razer-control-gui.desktop
install -Dm0644 packaging/icons/razer-control-gui.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/razer-control-gui.svg
install -Dm0644 udev/70-razer-control-secureblue.rules %{buildroot}%{_udevrulesdir}/70-razer-control-secureblue.rules
install -Dm0644 systemd/razer-control.socket %{buildroot}%{_userunitdir}/razer-control.socket
install -Dm0644 systemd/razer-control.service %{buildroot}%{_userunitdir}/razer-control.service

%post
%udev_rules_update

%postun
%udev_rules_update

%files
%license LICENSE
%doc README.md
%{_bindir}/razer-control
%{_bindir}/razer-control-gui
%{_bindir}/razer-control-tray
%{_datadir}/applications/razer-control-gui.desktop
%{_datadir}/icons/hicolor/scalable/apps/razer-control-gui.svg
%{_udevrulesdir}/70-razer-control-secureblue.rules
%{_userunitdir}/razer-control.socket
%{_userunitdir}/razer-control.service

%changelog
* Sun Jul 13 2026 razer-control-secureblue maintainers <llmplayerx@gmail.com> - 0.1.0-1
- Initial package: policy layer, dry-run socket-activated user daemon,
  uaccess udev rule, systemd user units.
