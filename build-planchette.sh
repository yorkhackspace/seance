#!/usr/bin/env sh

set -e pipefail

outputPath="planchette-deb/usr/bin/planchette"
rm -f $outputPath

nix build .#cross-armv6l-linux
cp result/bin/planchette $outputPath
chown $USER:$GROUP $outputPath
chmod 755 $outputPath
dpkg-deb --build planchette-deb planchette.deb
