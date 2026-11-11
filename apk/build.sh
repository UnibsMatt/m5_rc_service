#!/bin/sh
# Build the gw-monitor apk. Usage: ./build.sh   (output: ~/packages/apk/<arch>/)
set -eu
cd "$(dirname "$0")"
srcdir=""; . ./APKBUILD
crate=../gw-monitor

# signing key (once per build host)
if ! grep -qs PACKAGER_PRIVKEY "$HOME/.abuild/abuild.conf"; then
	PACKAGER="${PACKAGER:-gw-monitor builder <builder@localhost>}" abuild-keygen -a -n
fi
# trust our own key locally (copy the .pub to /etc/apk/keys on every gateway too)
. "$HOME/.abuild/abuild.conf"
pub="$PACKAGER_PRIVKEY.pub"
if [ ! -f "/etc/apk/keys/$(basename "$pub")" ]; then
	if [ "$(id -u)" = 0 ]; then cp "$pub" /etc/apk/keys/; else echo "install $pub into /etc/apk/keys as root"; fi
fi

# source tarball from the crate (without build output)
tar -czf "$pkgname-$pkgver.tar.gz" --exclude=./target \
	--transform "s,^\.,$pkgname-$pkgver," -C "$crate" .

ABUILD="abuild"; [ "$(id -u)" = 0 ] && ABUILD="abuild -F"
$ABUILD checksum
$ABUILD -r
ls -l "$HOME/packages/$(basename "$(cd .. && pwd)")/$(apk --print-arch)/$pkgname-$pkgver-r$pkgrel.apk"
