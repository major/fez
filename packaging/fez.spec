%global crate fez

Name:           fez
Version:        0.1.0
Release:        %autorelease
Summary:        Agent-native management CLI for Fedora/RHEL (drives cockpit-bridge)

License:        Apache-2.0
URL:            https://github.com/major-hayden/fez
# Source0 is the crate subtree with the crate at its root (the upstream repo
# keeps the crate under fez/). Produced from the repository root, e.g.
#   git archive --prefix=%%{crate}-%%{version}/ -o %%{crate}-%%{version}.tar.gz HEAD:fez
# Packit builds the identical archive via its create-archive action.
Source0:        %{crate}-%{version}.tar.gz
# Vendored dependencies (cargo vendor), bundled per the Fedora application
# bundling allowance. Regenerate with packaging/make-vendor.sh.
Source1:        %{crate}-%{version}-vendor.tar.xz

# Rust application: builds with the cargo RPM macros against the vendored tree.
BuildRequires:  cargo-rpm-macros >= 24

# Substrate and remote transport are runtime requirements (design Sections 5, 7).
Requires:       cockpit-bridge
Requires:       openssh-clients

%global _description %{expand:
fez gives LLM-driven agents a uniform, structured, discoverable way to operate a
Fedora/RHEL host (and, over SSH, a fleet) by driving cockpit-bridge over its
framed JSON protocol and reusing Cockpit's privilege escalation. It exposes
systemd services and journal logs with a versioned JSON envelope (fez/v1) and an
on-demand capability discovery model, plus an MCP gateway for MCP-aware agents.}

%description %{_description}

%prep
# -a1 extracts the vendor tarball (Source1) into the source dir as vendor/.
%autosetup -n %{crate}-%{version} -p1 -a1
%cargo_prep -v vendor

%build
%cargo_build
# Generate the man page from the built binary's hidden `man` subcommand so it
# stays in lockstep with the capability registry (long descriptions, examples).
$(find target -maxdepth 2 -type f -name fez | head -1) man > fez.1

%install
%cargo_install
# fez-fake-bridge is a test fixture, not a shipped artifact.
rm -f %{buildroot}%{_bindir}/fez-fake-bridge
# Application package: ship only the binary. cargo install also stages the
# crate registry source for library reuse, which a leaf app must not ship.
rm -rf %{buildroot}%{_datadir}/cargo
install -Dpm0644 fez.1 %{buildroot}%{_mandir}/man1/fez.1

%check
%cargo_test

%files
%license LICENSE
%doc README.md
%{_bindir}/fez
%{_mandir}/man1/fez.1*

%changelog
%autochangelog
