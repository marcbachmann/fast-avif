# Thanks

fast-avif is built on the work of these projects.

## rav1d

The Rust port of dav1d that the decoder, the tables and the DSP come from. The Internet
Security Research Group (ISRG) and its donors funded it through Prossimo.

The rav1d Rust port authors:

Khyber Sen, Nicole L, Per Larsen, Stephen Crane, Frank Bossen, Folkert de Vries

## dav1d

The AV1 decoder that rav1d ports, by VideoLAN and the dav1d authors, with funding from the
Alliance for Open Media (AOM). The dav1d project and the VideoLAN association thank:

- Two Orioles LLC, for important coding effort
- VideoLabs SAS
- the VideoLAN, FFmpeg and libplacebo projects

And all the dav1d Authors (git shortlog -sn), including:

Martin Storsjö, Henrik Gramner, Ronald S. Bultje, Janne Grunau, James Almer,
Victorien Le Couviour--Tuffet, Matthias Dressel, Marvin Scholz, Luc Trudeau,
Jean-Baptiste Kempf, Hugo Beauzée-Luyssen, Niklas Haas, Konstantin Pavlov,
David Michael Barr, Steve Lhomme, Nathan E. Egge, Kyle Siefring, Raphaël Zumer,
B Krishnan Iyer, Francois Cartegnie, Liwei Wang, Derek Buitenhuis,
Michael Bradshaw, Wan-Teh Chang, Xuefeng Jiang, Luca Barbato, Jan Beich,
Christophe Gisquet, Justin Bull, Boyuan Xiao, Dale Curtis, Emmanuel Gil Peyrot,
Rupert Swarbrick, Thierry Foucu, Thomas Daede, Colin Lee, Jonathan Wright,
Lynne, Michail Alvanos, Nico Weber, Salome Thirot, SmilingWolf, Tristan Laurent,
Vittorio Giovara, Yannis Guyon, André Kempe, Anisse Astier, Anton Mitrofanov,
Dmitriy Sychov, Ewout ter Hoeven, Fred Barbier, Jean-Yves Avenard, Joe Drago,
Mark Shuttleworth, Matthieu Bouron, Mehdi Sabwat, Nicolas Frattaroli,
Pablo Stebler, Rostislav Pehlivanov, Shiz, Steinar Midtskogen, Sylvain BERTRAND,
Sylvestre Ledru, Timo Gurr, Tristan Matthews, Vibhoothi, Xavier Claessens,
Xu Guangxin, kossh1 and skal

## libaom

The AOM reference encoder, v3.15.1, by the Alliance for Open Media and the libaom authors. The
encoder ports several of its parts:
- the range encoder;
- the forward transforms;
- the intra CNN partition prune and its model;
- λ and tune IQ's settings.

The ported files keep its notice.

## rav1e

The rav1e contributors' forward transforms, taken through rusty-av1-toolkit, are the encoder's
`fdct_fast` kernels. The ported files keep their headers.

## dav1d-rs

The Rust bindings to dav1d that the decoder's Rust API started from, under the MIT licence; the
notice is in `crates/dec/src/rust_api.rs`.
