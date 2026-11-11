#!/bin/sh
# Generate a development PKI for the ingest port.
#   ./gen-dev-certs.sh [extra SAN ...]      e.g. ./gen-dev-certs.sh IP:192.168.1.88 DNS:fleet.lan
# Produces (in this directory):
#   server.crt/.key  ingest TLS cert, signed by the dev CA (re-issued on every run)
#   ca.crt           dev CA -> copy to gateways and set API_CA_FILE to it
#   client-ca.crt    CAs trusted for gateway client certs (dev CA; append the iotplt Root CA)
#   pc-dev.crt/.key  a dev gateway client cert for testing
set -eu
cd "$(dirname "$0")"
umask 077
DAYS=825
SAN="DNS:localhost,IP:127.0.0.1"
for s in "$@"; do SAN="$SAN,$s"; done
tmp=$(mktemp -d)
trap 'rm -r "$tmp"' EXIT

if [ ! -f ca.key ]; then
	openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 3650 \
		-subj "/O=gw-fleet dev/CN=gw-fleet dev CA" -keyout ca.key -out ca.crt \
		-addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
		-addext "keyUsage=critical,keyCertSign,cRLSign" \
		-addext "subjectKeyIdentifier=hash" 2>/dev/null
fi
# leaf extensions (strict X.509 clients, e.g. Python >= 3.13, require KU + AKI)
LEAF="basicConstraints=critical,CA:FALSE
subjectKeyIdentifier=hash
authorityKeyIdentifier=keyid,issuer"

openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -subj "/CN=gw-fleet ingest" \
	-keyout server.key -out "$tmp/server.csr" 2>/dev/null
printf "%s\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=%s\n" "$LEAF" "$SAN" > "$tmp/server.ext"
openssl x509 -req -in "$tmp/server.csr" -CA ca.crt -CAkey ca.key -CAcreateserial -days $DAYS \
	-extfile "$tmp/server.ext" -out server.crt 2>/dev/null

openssl req -newkey rsa:2048 -nodes -subj "/CN=pc-dev" -keyout pc-dev.key -out "$tmp/pc-dev.csr" 2>/dev/null
printf "%s\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=clientAuth\n" "$LEAF" > "$tmp/client.ext"
openssl x509 -req -in "$tmp/pc-dev.csr" -CA ca.crt -CAkey ca.key -CAcreateserial -days $DAYS \
	-extfile "$tmp/client.ext" -out pc-dev.crt 2>/dev/null

[ -f client-ca.crt ] || cp ca.crt client-ca.crt
chmod 644 ca.crt client-ca.crt server.crt pc-dev.crt server.key   # nginx reads server.key in the container
echo "server SAN: $SAN"
ls -l
