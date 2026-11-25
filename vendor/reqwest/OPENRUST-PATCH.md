# OpenRust reqwest 0.13.5

Source is the crates.io reqwest 0.13.5 release at upstream commit de55373434f07f42926599dbb5a88550d8e55112, under the upstream MIT or Apache-2.0 licenses.

The proxy module supplies an opt-in numeric CONNECT destination. The connection module uses that address only for the HTTP proxy tunnel. The origin URI remains the source of the target TLS identity. Numeric CONNECT supports plain HTTP proxies or HTTPS proxies with opt-in, independent Rustls root certificates and server name. The TLS proxy connection uses a new config without client authentication. The endpoint client certificate is available only to the origin TLS connection. Default upstream proxy behavior is unchanged.
