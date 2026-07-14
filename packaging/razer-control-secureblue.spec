# CI-oriented spec: builds with network access for crates.io, the npm registry,
# and (via next/font/google) Google's font CDN.  An offline COPR build needs all
# three vendored; see docs/INSTALL.md, Path C.  Tracked for the packaging
# milestone.

Name:           razer-control-secureblue
Version:        0.1.0
Release:        1%{?dist}
Summary:        Safety-first Razer Blade control for Atomic Linux
License:        GPL-2.0-only
URL:            https://github.com/cyph3rpuNk-dev/razer-control-secureblue
Source0:        %{name}-%{version}.tar.gz

BuildRequires:  cargo
BuildRequires:  rust
BuildRequires:  gcc
BuildRequires:  systemd-rpm-macros
# ksni (StatusNotifierItem tray) links libdbus
BuildRequires:  dbus-devel
# Tauri desktop shell: WebKitGTK webview and its GTK stack
BuildRequires:  webkit2gtk4.1-devel
BuildRequires:  gtk3-devel
BuildRequires:  librsvg2-devel
BuildRequires:  openssl-devel
# The Tauri binary embeds the Next.js static export, so the frontend is built
# here, before cargo compiles the shell around it.
BuildRequires:  nodejs
BuildRequires:  nodejs-npm

Requires:       webkit2gtk4.1

%description
Per-user, socket-activated control daemon for Razer Blade laptops, built for
Fedora Atomic and Secureblue. Session-scoped uaccess HID permissions instead
of world-writable device nodes, a verified per-model capability table, and an
automatic-fan failsafe. The current release contains the policy layer and a
dry-run daemon; it sends no hardware commands.

%prep
%autosetup

%build
export NEXT_TELEMETRY_DISABLED=1
npm install -g pnpm@11
pushd webui
pnpm install --frozen-lockfile
# Produces webui/out, which tauri::generate_context! embeds into the binary.
pnpm build
popd
cargo build --release --locked \
  -p razer-control-secureblue -p razer-control-tray -p razer-control-desktop

%check
cargo test --release --locked \
  -p razer-control-secureblue -p razer-control-tray -p razer-control-desktop

%install
install -Dm0755 target/release/razer-control %{buildroot}%{_bindir}/razer-control
install -Dm0755 target/release/razer-control-desktop %{buildroot}%{_bindir}/razer-control-desktop
install -Dm0755 target/release/razer-control-tray %{buildroot}%{_bindir}/razer-control-tray
install -Dm0644 packaging/razer-control-desktop.desktop %{buildroot}%{_datadir}/applications/razer-control-desktop.desktop
install -Dm0644 packaging/icons/razer-control-desktop.svg %{buildroot}%{_datadir}/icons/hicolor/scalable/apps/razer-control-desktop.svg
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
%{_bindir}/razer-control-desktop
%{_bindir}/razer-control-tray
%{_datadir}/applications/razer-control-desktop.desktop
%{_datadir}/icons/hicolor/scalable/apps/razer-control-desktop.svg
%{_udevrulesdir}/70-razer-control-secureblue.rules
%{_userunitdir}/razer-control.socket
%{_userunitdir}/razer-control.service

%changelog
* Tue Jul 14 2026 razer-control-secureblue maintainers <llmplayerx@gmail.com> - 0.1.0-1
- Initial package: policy layer, dry-run socket-activated user daemon,
  uaccess udev rule, systemd user units, KDE tray, and the Tauri desktop app.
