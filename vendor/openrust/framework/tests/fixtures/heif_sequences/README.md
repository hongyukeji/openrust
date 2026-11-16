# HEIF sequence metadata fixtures

These synthetic inputs contain 64×48 red, green, and blue frames at
40, 90, and 180 milliseconds, respectively. FFmpeg encoded the AVIF samples
with SVT-AV1 and the HEIC samples with x265. The AVIF plays three times and
the HEIC plays indefinitely.

The fixtures use three visual sample descriptions with different sRGB ICC
manufacturer signatures, `FRM0` through `FRM2`. Two timed metadata tracks
describe the visual track through `cdsc` references. Their EXIF Artist tags
and XMP packets identify the corresponding frame number. The ICC profiles
and EXIF payloads were generated using Pillow and Little CMS.
