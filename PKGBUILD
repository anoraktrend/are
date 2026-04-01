# Maintainer: Lucy Randall <lucybrown@vivaldi.net>
pkgname=are
pkgver=1.0.0
pkgrel=1
pkgdesc="Another Rust Editor - A port of the aee terminal-based text editor to Rust"
arch=('x86_64' 'i686' 'aarch64')
url="https://github.com/lucy/are"
license=('Artistic-2.0')
depends=()
makedepends=('cargo')
provides=('are')
conflicts=('are')
source=("${pkgname}::git+https://github.com/anoraktrend/are.git#tag=v${pkgver}")
sha256sums=('SKIP')

build() {
    cd "$pkgname"
    cargo build --release --locked
}

package() {
    cd "$pkgname"
    # Install the binary
    install -Dm755 "target/release/are" "$pkgdir/usr/bin/are"

    # Install license
    install -Dm644 "LICENSE.MD" "$pkgdir/usr/share/licenses/$pkgname/LICENSE.MD"

    # Install documentation
    install -Dm644 "README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
    install -Dm644 "Changes" "$pkgdir/usr/share/doc/$pkgname/Changes"
}
