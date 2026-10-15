.DEFAULT_GOAL := build

PREFIX ?= /usr
BINDIR ?= ${PREFIX}/bin
SBINDIR ?= ${PREFIX}/sbin
# The location of the helper is compiled into the tools which call
# it, see HELPER in common/src/defaults.rs
LIBEXECDIR ?= /usr/libexec/flake-pilot
SHAREDIR ?= ${PREFIX}/share/podman-pilot
COMPLETIONDIR ?= ${PREFIX}/share/bash-completion/completions
TEMPLATEDIR ?= /etc/flakes
# The location of the meta data directory configuration is compiled
# into the tools, see META_DIR_TMPFILES_CONF in common/src/defaults.rs
TMPFILESDIR ?= /usr/lib/tmpfiles.d
DRACUTDIR ?= ${PREFIX}/lib/dracut
MANDIR ?= ${PREFIX}/share/man

# Set to yes to build a statically linked sci, requires the static
# glibc library
SCI_STATIC ?= no
# Set to yes to let systemd-tmpfiles create the podman and firecracker
# state directories below /var/lib instead of installing them
TMPFILES_STATEDIRS ?= no

ARCH = $(shell uname -m)

.PHONY: package
package: clean vendor sourcetar
	rm -rf package/build
	mkdir -p package/build
	gzip package/flake-pilot.tar
	mv package/flake-pilot.tar.gz package/build
	cp package/flake-pilot.spec package/build
	cp package/flake-pilot-rpmlintrc package/build
	# update changelog using reference file
	helper/update_changelog.py --since package/flake-pilot.changes.ref > \
		package/build/flake-pilot.changes
	helper/update_changelog.py --file package/flake-pilot.changes.ref >> \
		package/build/flake-pilot.changes
	@echo "Find package data at package/build"

vendor:
	rm -rf vendor
	cargo vendor-filterer --platform=*-unknown-linux-gnu

sourcetar:
	rm -rf package/flake-pilot
	mkdir package/flake-pilot
	cp Makefile package/flake-pilot
	cp flakes.yml package/flake-pilot
	cp -a completion package/flake-pilot/
	cp -a common package/flake-pilot/
	cp -a podman-pilot package/flake-pilot/
	cp -a bubblewrap-pilot package/flake-pilot/
	cp -a flake-ctl package/flake-pilot/
	cp -a flake-pilot-helper package/flake-pilot/
	cp -a firecracker-pilot package/flake-pilot/
	cp -a doc package/flake-pilot/
	cp -a utils package/flake-pilot/
	cp -a vendor package/flake-pilot
	cp Cargo.toml package/flake-pilot
	# build from the vendored sources
	mkdir package/flake-pilot/.cargo
	cp package/cargo_config package/flake-pilot/.cargo/config.toml
	# systemd-tmpfiles configurations installed by make install
	mkdir package/flake-pilot/package
	cp package/systemd-tmpfiles-*.conf package/flake-pilot/package

	tar -C package -cf package/flake-pilot.tar flake-pilot
	rm -rf package/flake-pilot

.PHONY:build
build: compile man
ifeq ($(SCI_STATIC),yes)
	${MAKE} compile_sci_static
endif

compile:
	cargo build -v --release

compile_sci_static:
	cd firecracker-pilot/guestvm-tools/sci && RUSTFLAGS='-C target-feature=+crt-static' cargo build -v --profile static --target $(ARCH)-unknown-linux-gnu

clean:
	cd common && cargo -v clean
	cd podman-pilot && cargo -v clean
	cd bubblewrap-pilot && cargo -v clean
	cd firecracker-pilot && cargo -v clean
	cd flake-ctl && cargo -v clean
	cd flake-pilot-helper && cargo -v clean
	cd firecracker-pilot/guestvm-tools/sci && cargo -v clean
	rm -rf common/vendor
	rm -rf podman-pilot/vendor
	rm -rf bubblewrap-pilot/vendor
	rm -rf flake-ctl/vendor
	rm -rf flake-pilot-helper/vendor
	rm -rf firecracker-pilot/guestvm-tools/sci/vendor
	rm -rf package/build
	${MAKE} -C doc clean
	$(shell find . -name Cargo.lock | xargs rm -f)
	$(shell find . -type d -name vendor | xargs rm -rf)

test:
	cargo -v test

install:
	install -d -m 755 $(DESTDIR)$(BINDIR)
	install -d -m 755 $(DESTDIR)$(SBINDIR)
	install -d -m 755 $(DESTDIR)$(LIBEXECDIR)
	install -d -m 755 $(DESTDIR)$(SHAREDIR)
	install -d -m 755 $(DESTDIR)$(COMPLETIONDIR)
	install -d -m 755 $(DESTDIR)$(TEMPLATEDIR)
	install -d -m 755 $(DESTDIR)$(MANDIR)/man8
	install -d -m 755 $(DESTDIR)$(TMPFILESDIR)
	install -d -m 755 $(DESTDIR)$(DRACUTDIR)/modules.d/80netstart
	install -d -m 755 $(DESTDIR)/etc/dracut.conf.d
	install -d -m 755 $(DESTDIR)/overlayroot
	install -m 755 target/release/podman-pilot \
		$(DESTDIR)$(BINDIR)/podman-pilot
	install -m 755 target/release/firecracker-pilot \
		$(DESTDIR)$(BINDIR)/firecracker-pilot
	install -m 755 target/release/bubblewrap-pilot \
		$(DESTDIR)$(BINDIR)/bubblewrap-pilot
	install -m 755 target/release/flake-ctl \
		$(DESTDIR)$(BINDIR)/flake-ctl
	install -m 755 target/release/flake-pilot-helper \
		$(DESTDIR)$(LIBEXECDIR)/flake-pilot-helper
	install -m 644 flake-ctl/template/container-flake.yaml \
		$(DESTDIR)$(TEMPLATEDIR)/container-flake.yaml
	install -m 644 flake-ctl/template/firecracker-flake.yaml \
		$(DESTDIR)$(TEMPLATEDIR)/firecracker-flake.yaml
	install -m 644 flake-ctl/template/bubblewrap-flake.yaml \
		$(DESTDIR)$(TEMPLATEDIR)/bubblewrap-flake.yaml
	install -m 644 firecracker-pilot/template/firecracker.json \
		$(DESTDIR)$(TEMPLATEDIR)/firecracker.json
	install -m 644 podman-pilot/registry/storage.conf \
		$(DESTDIR)$(TEMPLATEDIR)/storage.conf
	install -m 644 flakes.yml $(DESTDIR)/etc/flakes.yml
	install -m 644 doc/*.8 $(DESTDIR)$(MANDIR)/man8
	install -m 755 utils/* $(DESTDIR)$(SBINDIR)
	# sci, prefer the static build if present
	if [ -f target/$(ARCH)-unknown-linux-gnu/static/sci ]; then \
		${MAKE} install_sci_static; \
	else \
		${MAKE} install_sci; \
	fi
	# completion
	install -m 755 completion/flake-ctl \
		$(DESTDIR)$(COMPLETIONDIR)
	# meta data directory shared by the pilots of all users
	install -m 644 package/systemd-tmpfiles-flake-pilot.conf \
		$(DESTDIR)$(TMPFILESDIR)/flake-pilot.conf
	# podman and firecracker state directories
ifeq ($(TMPFILES_STATEDIRS),yes)
	install -m 644 package/systemd-tmpfiles-firecracker-for-suse.conf \
		$(DESTDIR)$(TMPFILESDIR)/flake-pilot-firecracker.conf
	install -m 644 package/systemd-tmpfiles-podman-for-suse.conf \
		$(DESTDIR)$(TMPFILESDIR)/flake-pilot-podman.conf
else
	install -d -m 755 $(DESTDIR)/var/lib/firecracker/images
	install -d -m 755 $(DESTDIR)/var/lib/firecracker/storage
	install -d -m 755 $(DESTDIR)/var/lib/podman
	install -d -m 700 $(DESTDIR)/var/lib/podman/storage
endif
	# dracut network startup module for firecracker guests
	install -m 644 \
		firecracker-pilot/dracut/usr/lib/dracut/modules.d/80netstart/module-setup.sh \
		$(DESTDIR)$(DRACUTDIR)/modules.d/80netstart
	install -m 755 \
		firecracker-pilot/dracut/usr/lib/dracut/modules.d/80netstart/netstart.sh \
		$(DESTDIR)$(DRACUTDIR)/modules.d/80netstart
	install -m 644 \
		firecracker-pilot/dracut/etc/dracut.conf.d/extramodules.conf \
		$(DESTDIR)/etc/dracut.conf.d/extramodules.conf

install_sci_static:
	install -m 755 target/$(ARCH)-unknown-linux-gnu/static/sci \
		$(DESTDIR)$(SBINDIR)/sci

install_sci:
	install -m 755 target/release/sci \
		$(DESTDIR)$(SBINDIR)/sci

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/flake-ctl
	rm -f $(DESTDIR)$(BINDIR)/podman-pilot
	rm -f $(DESTDIR)$(BINDIR)/firecracker-pilot
	rm -f $(DESTDIR)$(BINDIR)/bubblewrap-pilot
	rm -f $(DESTDIR)$(LIBEXECDIR)/flake-pilot-helper
	rm -f $(DESTDIR)$(SBINDIR)/sci
	rm -f $(DESTDIR)$(SBINDIR)/flake-registry
	rm -f $(DESTDIR)$(COMPLETIONDIR)/flake-ctl
	rm -f $(DESTDIR)/etc/flakes.yml
	rm -f $(DESTDIR)$(TMPFILESDIR)/flake-pilot.conf
	rm -f $(DESTDIR)$(TMPFILESDIR)/flake-pilot-firecracker.conf
	rm -f $(DESTDIR)$(TMPFILESDIR)/flake-pilot-podman.conf
	rm -f $(DESTDIR)/etc/dracut.conf.d/extramodules.conf
	rm -rf $(DESTDIR)$(DRACUTDIR)/modules.d/80netstart
	rm -rf $(DESTDIR)$(SHAREDIR) $(DESTDIR)$(TEMPLATEDIR)

man:
	${MAKE} -C doc man

user_guide:
	${MAKE} -C doc user_guide

cargo:
	for path in $(shell find . -name Cargo.toml ! -path "*/vendor/*");do \
		pushd `dirname $$path`; cargo build || exit 1; popd;\
	done

setup:
	poetry install --all-extras

docs: setup
	poetry run make -C doc user_guide

ghpages: setup
	poetry run sphinx-build doc/user_guide doc/build
