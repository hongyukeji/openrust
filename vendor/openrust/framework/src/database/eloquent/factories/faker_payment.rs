// IBAN layouts from FakerPHP 1.24.1, src/Faker/Provider/Payment.php.
// Copyright (c) 2011 François Zaninotto
// Portions Copyright (c) 2008 Caius Durling
// Portions Copyright (c) 2008 Adam Royle
// Portions Copyright (c) 2008 Fiona Burrows
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

pub(super) const IBAN_FORMATS: &[(&str, &[u8])] = &[
    ("AD", b"nnnnnnnncccccccccccc"),
    ("AE", b"nnnnnnnnnnnnnnnnnnn"),
    ("AL", b"nnnnnnnncccccccccccccccc"),
    ("AT", b"nnnnnnnnnnnnnnnn"),
    ("AZ", b"aaaacccccccccccccccccccc"),
    ("BA", b"nnnnnnnnnnnnnnnn"),
    ("BE", b"nnnnnnnnnnnn"),
    ("BG", b"aaaannnnnncccccccc"),
    ("BH", b"aaaacccccccccccccc"),
    ("BR", b"nnnnnnnnnnnnnnnnnnnnnnnac"),
    ("CH", b"nnnnncccccccccccc"),
    ("CR", b"nnnnnnnnnnnnnnnnnn"),
    ("CY", b"nnnnnnnncccccccccccccccc"),
    ("CZ", b"nnnnnnnnnnnnnnnnnnnn"),
    ("DE", b"nnnnnnnnnnnnnnnnnn"),
    ("DK", b"nnnnnnnnnnnnnn"),
    ("DO", b"ccccnnnnnnnnnnnnnnnnnnnn"),
    ("EE", b"nnnnnnnnnnnnnnnn"),
    ("EG", b"nnnnnnnnnnnnnnnnnnnnnnnnn"),
    ("ES", b"nnnnnnnnnnnnnnnnnnnn"),
    ("FI", b"nnnnnnnnnnnnnn"),
    ("FR", b"nnnnnnnnnncccccccccccnn"),
    ("GB", b"aaaannnnnnnnnnnnnn"),
    ("GE", b"aannnnnnnnnnnnnnnn"),
    ("GI", b"aaaaccccccccccccccc"),
    ("GR", b"nnnnnnncccccccccccccccc"),
    ("GT", b"cccccccccccccccccccccccc"),
    ("HR", b"nnnnnnnnnnnnnnnnn"),
    ("HU", b"nnnnnnnnnnnnnnnnnnnnnnnn"),
    ("IE", b"aaaannnnnnnnnnnnnn"),
    ("IL", b"nnnnnnnnnnnnnnnnnnn"),
    ("IS", b"nnnnnnnnnnnnnnnnnnnnnn"),
    ("IT", b"annnnnnnnnncccccccccccc"),
    ("KW", b"aaaannnnnnnnnnnnnnnnnnnnnn"),
    ("KZ", b"nnnccccccccccccc"),
    ("LB", b"nnnncccccccccccccccccccc"),
    ("LI", b"nnnnncccccccccccc"),
    ("LT", b"nnnnnnnnnnnnnnnn"),
    ("LU", b"nnnccccccccccccc"),
    ("LV", b"aaaaccccccccccccc"),
    ("MC", b"nnnnnnnnnncccccccccccnn"),
    ("MD", b"cccccccccccccccccccc"),
    ("ME", b"nnnnnnnnnnnnnnnnnn"),
    ("MK", b"nnnccccccccccnn"),
    ("MR", b"nnnnnnnnnnnnnnnnnnnnnnn"),
    ("MT", b"aaaannnnncccccccccccccccccc"),
    ("MU", b"aaaannnnnnnnnnnnnnnnnnnaaa"),
    ("NL", b"aaaannnnnnnnnn"),
    ("NO", b"nnnnnnnnnnn"),
    ("PK", b"aaaacccccccccccccccc"),
    ("PL", b"nnnnnnnnnnnnnnnnnnnnnnnn"),
    ("PS", b"aaaaccccccccccccccccccccc"),
    ("PT", b"nnnnnnnnnnnnnnnnnnnnn"),
    ("RO", b"aaaacccccccccccccccc"),
    ("RS", b"nnnnnnnnnnnnnnnnnn"),
    ("SA", b"nncccccccccccccccccc"),
    ("SE", b"nnnnnnnnnnnnnnnnnnnn"),
    ("SI", b"nnnnnnnnnnnnnnn"),
    ("SK", b"nnnnnnnnnnnnnnnnnnnn"),
    ("SM", b"annnnnnnnnncccccccccccc"),
    ("TN", b"nnnnnnnnnnnnnnnnnnnn"),
    ("TR", b"nnnnnncccccccccccccccc"),
    ("VG", b"aaaannnnnnnnnnnnnnnn"),
];
