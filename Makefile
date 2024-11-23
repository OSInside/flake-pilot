.DEFAULT_GOAL := build

PREFIX ?= /usr
BINDIR ?= ${PREFIX}/bin
SBINDIR ?= ${PREFIX}/sbin
SHAREDIR ?= ${PREFIX}/share/podman-pilot
COMPLETIONDIR ?= ${PREFIX}/share/bash-completion/completions
FLAKEDIR ?= ${PREFIX}/share/flakes
TEMPLATEDIR ?= /etc/flakes

ARCH = $(shell uname -m)

.PHONY: package
package: clean vendor sourcetar
	rm -rf package/build
	mkdir -p package/build
	gzip package/flake-pilot.tar
	mv package/flake-pilot.tar.gz package/build
	cp package/flake-pilot.spec package/build
	cp package/cargo_config package/build
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
	cp -a flake-ctl package/flake-pilot/
	cp -a firecracker-pilot package/flake-pilot/
	cp -a doc package/flake-pilot/
	cp -a utils package/flake-pilot/
	cp -a vendor package/flake-pilot
	cp Cargo.toml package/flake-pilot

	tar -C package -cf package/flake-pilot.tar flake-pilot
	rm -rf package/flake-pilot

.PHONY:build
build: compile man

compile:
	cargo build -v --release

compile_sci_static:
	cd firecracker-pilot/guestvm-tools/sci && RUSTFLAGS='-C target-feature=+crt-static' cargo build -v --profile static --target $(ARCH)-unknown-linux-gnu

clean:
	cd common && cargo -v clean
	cd podman-pilot && cargo -v clean
	cd firecracker-pilot && cargo -v clean
	cd flake-ctl && cargo -v clean
	cd firecracker-pilot/guestvm-tools/sci && cargo -v clean
	rm -rf common/vendor
	rm -rf podman-pilot/vendor
	rm -rf flake-ctl/vendor
	rm -rf firecracker-pilot/guestvm-tools/sci/vendor
	rm -rf package/build
	${MAKE} -C doc clean
	$(shell find . -name Cargo.lock | xargs rm -f)
	$(shell find . -type d -name vendor | xargs rm -rf)

test:
	cd podman-pilot && cargo -v build
	cd podman-pilot && cargo -v test

install:
	install -d -m 755 $(DESTDIR)$(BINDIR)
	install -d -m 755 $(DESTDIR)$(SBINDIR)
	install -d -m 755 $(DESTDIR)$(SHAREDIR)
	install -d -m 755 $(DESTDIR)$(COMPLETIONDIR)
	install -d -m 755 $(DESTDIR)$(TEMPLATEDIR)
	install -d -m 755 $(DESTDIR)$(FLAKEDIR)
	install -d -m 755 ${DESTDIR}/usr/share/man/man8
	install -m 755 target/release/podman-pilot \
		$(DESTDIR)$(BINDIR)/podman-pilot
	install -m 755 target/release/firecracker-pilot \
		$(DESTDIR)$(BINDIR)/firecracker-pilot
	install -m 755 target/release/flake-ctl \
		$(DESTDIR)$(BINDIR)/flake-ctl
	install -m 644 flake-ctl/template/container-flake.yaml \
		$(DESTDIR)$(TEMPLATEDIR)/container-flake.yaml
	install -m 644 flake-ctl/template/firecracker-flake.yaml \
		$(DESTDIR)$(TEMPLATEDIR)/firecracker-flake.yaml
	install -m 644 firecracker-pilot/template/firecracker.json \
		$(DESTDIR)$(TEMPLATEDIR)/firecracker.json
	install -m 644 podman-pilot/registry/storage.conf \
		$(DESTDIR)$(TEMPLATEDIR)/storage.conf
	install -m 644 doc/*.8 ${DESTDIR}/usr/share/man/man8
	install -m 755 utils/* $(DESTDIR)$(SBINDIR)
	# completion
	install -m 755 completion/flake-ctl \
		$(DESTDIR)$(COMPLETIONDIR)

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
	rm -rf $(DESTDIR)$(FLAKEDIR) $(DESTDIR)$(SHAREDIR) $(DESTDIR)$(TEMPLATEDIR)

man:
	${MAKE} -C doc man

cargo:
	for path in $(shell find . -name Cargo.toml ! -path "*/vendor/*");do \
		pushd `dirname $$path`; cargo build || exit 1; popd;\
	done
