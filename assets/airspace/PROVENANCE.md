# Airspace silhouettes

The nine aircraft silhouettes in `src/airspace_silhouettes.cpp` (Unknown, Light,
Small, Large, Heavy, Fast, Helicopter, Glider, Balloon) were drawn for Newt from
scratch in October 2026 for #166, as integer outline points, quadratic control
points, bars, discs and cut-out windows in one 600 x 240 unit box, with
simpler small forms of two classes for list rows. They are original project
work, like the rest of this repository's own code: no third-party artwork,
font, photograph, tracing, reference image or network input was used to make
them, and no image file is their source. The code is the only source.

Each drawing is a generic side view of a class of aircraft, nose to the right,
not a model of any one aircraft type, operator or livery. The class comes only
from the ADS-B emitter category the provider reports, never from the ICAO type
code.

Every size is rasterized at run time from the same drawing (`drawSilhouette`):
an even-odd scanline fill at pixel centres in exact integer units, then a 2 x 2
opening so no size has one-pixel slivers. No bitmap is stored. Host tests
(`tests/airspace_render_test.cpp`) check every class at each size Airspace draws.
