# Maintainer: Your Name <lucyrandall@helltop.net>
pkgname=aee-rust
pkgver=2.2.22.r25.c77b92a
pkgrel=1
pkgdesc="Another Easy Editor - a simple, easy to use terminal-based screen oriented editor"
arch=('x86_64' 'i686' 'aarch64')
url="https://helltop.net/projects"
license=('custom:Artistic')
depends=('ncurses')
makedepends=('git' 'cargo')
provides=('aee')
conflicts=('aee')
source=("git+https://github.com/anoraktrend/aee.git")
sha256sums=('SKIP')

pkgver() {
    cd "$srcdir/aee"
    printf "2.2.22.r%s.%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
}

build() {
    cd "$srcdir/aee"
    cargo build --release
}

package() {
    cd "$srcdir/aee"

    # Install the binary
    install -Dm755 "target/release/aee" "$pkgdir/usr/bin/aee"

    # Install license
    install -Dm644 "LICENSE" "$pkgdir/usr/share/licenses/$pkgname/LICENSE"

    # Install documentation
    install -Dm644 "README.md" "$pkgdir/usr/share/doc/$pkgname/README.md"
}
