//! tests/node/crypto/ — 对齐 src/builtins/node/crypto.rs（按算法族分块；目录名 crypto 避与 crypto.rs 目标文件同名）。

#[path = "crypto/hash.rs"]
mod hash;
#[path = "crypto/cipher.rs"]
mod cipher;
#[path = "crypto/asym.rs"]
mod asym;
#[path = "crypto/parity.rs"]
mod parity;
#[path = "crypto/x509.rs"]
mod x509;
#[path = "crypto/pq.rs"]
mod pq;
