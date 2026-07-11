// Package optimize provides best-effort image compression before upload.
// It uses only pure-Go encoders (no CGO, no external binaries) so it behaves
// identically on every platform the release targets.
//
// Strategy:
//   - JPEG → re-encode at quality 85; keep the smaller of the two.
//   - PNG without an alpha channel → try both JPEG q85 and lossless WebP,
//     keep whichever is smallest (falling back to the original PNG).
//   - PNG with transparency → try lossless WebP (which preserves the alpha
//     channel); keep it only if smaller than the original PNG.
//   - All other formats (SVG, GIF, WebP, AVIF) → pass through unchanged.
//
// WebP encoding uses github.com/HugoSmits86/nativewebp, a pure-Go lossless
// (VP8L) encoder. Lossless WebP typically beats PNG by 15–25% on screenshots
// and UI graphics while preserving transparency.
package optimize

import (
	"bytes"
	"encoding/binary"
	"fmt"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"io"

	xdraw "golang.org/x/image/draw"

	"github.com/HugoSmits86/nativewebp"
)

const jpegQuality = 85

// Result is the output of TryCompress. If no compression gain was found,
// Body contains the original data rewound to the start.
type Result struct {
	Body        io.ReadSeeker
	ContentType string
	// Size is the byte length of Body. May equal OriginalSize if unchanged.
	Size int64
	// OriginalSize holds the pre-compression size for reporting purposes.
	OriginalSize int64
	// Reduced is true when the output is smaller than the input.
	Reduced bool
}

// TryCompress attempts to produce a smaller encoding of r.
// It always returns a valid Result: on any decoding error it silently falls
// back to the original data so that the caller can proceed with the upload.
func TryCompress(r io.ReadSeeker, contentType string, origSize int64) (Result, error) {
	passthrough := func() (Result, error) {
		if _, err := r.Seek(0, io.SeekStart); err != nil {
			return Result{}, fmt.Errorf("rewind for passthrough: %w", err)
		}
		return Result{Body: r, ContentType: contentType, Size: origSize, OriginalSize: origSize}, nil
	}

	switch contentType {
	case "image/jpeg":
		return reEncodeJPEG(r, origSize)
	case "image/png":
		res, err := pngOptimize(r, origSize)
		if err != nil {
			// Fall back silently — optimisation is best-effort.
			return passthrough()
		}
		return res, nil
	default:
		return passthrough()
	}
}

// reEncodeJPEG decodes a JPEG and re-encodes it at jpegQuality.
// Returns the original if re-encoding is larger or if decoding fails.
func reEncodeJPEG(r io.ReadSeeker, origSize int64) (Result, error) {
	if _, err := r.Seek(0, io.SeekStart); err != nil {
		return Result{}, fmt.Errorf("rewind JPEG: %w", err)
	}
	img, err := jpeg.Decode(r)
	if err != nil {
		// Decode failure → return original.
		if _, se := r.Seek(0, io.SeekStart); se != nil {
			return Result{}, se
		}
		return Result{Body: r, ContentType: "image/jpeg", Size: origSize, OriginalSize: origSize}, nil
	}
	var buf bytes.Buffer
	buf.Grow(int(origSize))
	if err := jpeg.Encode(&buf, img, &jpeg.Options{Quality: jpegQuality}); err != nil {
		if _, se := r.Seek(0, io.SeekStart); se != nil {
			return Result{}, se
		}
		return Result{Body: r, ContentType: "image/jpeg", Size: origSize, OriginalSize: origSize}, nil
	}
	if int64(buf.Len()) >= origSize {
		// No gain.
		if _, se := r.Seek(0, io.SeekStart); se != nil {
			return Result{}, se
		}
		return Result{Body: r, ContentType: "image/jpeg", Size: origSize, OriginalSize: origSize}, nil
	}
	b := buf.Bytes()
	return Result{
		Body:         bytes.NewReader(b),
		ContentType:  "image/jpeg",
		Size:         int64(len(b)),
		OriginalSize: origSize,
		Reduced:      true,
	}, nil
}

// pngOptimize chooses the smallest of: the original PNG, a JPEG re-encoding
// (opaque images only), and a lossless WebP re-encoding. WebP preserves the
// alpha channel, so transparent images can still be compressed.
func pngOptimize(r io.ReadSeeker, origSize int64) (Result, error) {
	if _, err := r.Seek(0, io.SeekStart); err != nil {
		return Result{}, fmt.Errorf("rewind PNG: %w", err)
	}
	img, err := png.Decode(r)
	if err != nil {
		return Result{}, fmt.Errorf("decode PNG: %w", err)
	}
	transparent := hasTransparency(img)

	// Track the best candidate found so far, starting with the original PNG.
	best := Result{ContentType: "image/png", Size: origSize, OriginalSize: origSize}
	var bestBytes []byte

	// JPEG candidate — only for fully opaque images (lossy, drops alpha).
	if !transparent {
		var buf bytes.Buffer
		buf.Grow(int(origSize))
		if err := jpeg.Encode(&buf, img, &jpeg.Options{Quality: jpegQuality}); err == nil {
			if int64(buf.Len()) < best.Size {
				bestBytes = append([]byte(nil), buf.Bytes()...)
				best = Result{ContentType: "image/jpeg", Size: int64(len(bestBytes)), OriginalSize: origSize, Reduced: true}
			}
		}
	}

	// WebP candidate — lossless, preserves transparency.
	var wbuf bytes.Buffer
	wbuf.Grow(int(origSize))
	if err := nativewebp.Encode(&wbuf, img, &nativewebp.Options{}); err == nil {
		if int64(wbuf.Len()) < best.Size {
			bestBytes = append([]byte(nil), wbuf.Bytes()...)
			best = Result{ContentType: "image/webp", Size: int64(len(bestBytes)), OriginalSize: origSize, Reduced: true}
		}
	}

	if best.Reduced {
		best.Body = bytes.NewReader(bestBytes)
		return best, nil
	}
	// Nothing beat the original PNG.
	if _, se := r.Seek(0, io.SeekStart); se != nil {
		return Result{}, se
	}
	return Result{Body: r, ContentType: "image/png", Size: origSize, OriginalSize: origSize}, nil
}

// hasTransparency reports whether img contains any non-opaque pixels.
// For performance it samples approximately 1 % of pixels (at least 1000,
// at most 10 000) rather than scanning the entire image.
func hasTransparency(img image.Image) bool {
	switch img.ColorModel() {
	case color.RGBAModel, color.RGBA64Model, color.NRGBAModel, color.NRGBA64Model:
		// These models can carry alpha; sample pixels.
	default:
		switch img.(type) {
		case *image.Paletted:
			// Check whether the palette contains any transparent colour.
			for _, c := range img.(*image.Paletted).Palette {
				_, _, _, a := c.RGBA()
				if a != 0xffff {
					return true
				}
			}
			return false
		default:
			// Gray, Gray16, YCbCr, CMYK — no alpha channel.
			return false
		}
	}

	bounds := img.Bounds()
	total := bounds.Dx() * bounds.Dy()
	if total == 0 {
		return false
	}
	// step: sample ~1% of pixels, clamped to [1000, 10000].
	step := total / 100
	if step < 1 {
		step = 1
	}
	if step > 10 {
		step = 10 // ensure we check at most ~10000 samples across a 100x100 grid
	}
	n := 0
	for y := bounds.Min.Y; y < bounds.Max.Y; y++ {
		for x := bounds.Min.X; x < bounds.Max.X; x++ {
			n++
			if n%step != 0 {
				continue
			}
			_, _, _, a := img.At(x, y).RGBA()
			if a != 0xffff {
				return true
			}
		}
	}
	return false
}

// ─── EXIF stripping ───────────────────────────────────────────────────────────

// StripJPEGMetadata removes EXIF, XMP, and other APP1 metadata markers from a
// JPEG byte slice without re-encoding (lossless). It returns the stripped data
// and true when metadata was found. For non-JPEG input it returns nil, false.
//
// Privacy note: APP1 markers carry EXIF data including GPS coordinates, device
// model, and timestamps embedded by cameras and smartphones.
func StripJPEGMetadata(data []byte) ([]byte, bool) {
	if len(data) < 4 || data[0] != 0xFF || data[1] != 0xD8 {
		return nil, false // not a JPEG
	}

	var out bytes.Buffer
	out.Grow(len(data))
	out.WriteByte(0xFF)
	out.WriteByte(0xD8) // SOI

	stripped := false
	i := 2
	for i < len(data) {
		if data[i] != 0xFF {
			// We are past SOS — image-compressed data, copy verbatim.
			out.Write(data[i:])
			break
		}
		if i+1 >= len(data) {
			break
		}
		marker := data[i+1]
		i += 2

		// Markers with no payload: RST0-RST7, SOI, EOI.
		if marker == 0xD8 || marker == 0xD9 || (marker >= 0xD0 && marker <= 0xD7) {
			out.WriteByte(0xFF)
			out.WriteByte(marker)
			if marker == 0xD9 {
				break
			}
			continue
		}
		if i+1 >= len(data) {
			break
		}
		// Segment length is big-endian and includes the 2 length bytes.
		segLen := int(binary.BigEndian.Uint16(data[i : i+2]))
		if i+segLen > len(data) {
			break
		}
		payload := data[i : i+segLen]
		i += segLen

		// Strip APP1 (EXIF/XMP). Keep APP0 (JFIF/JFXX) for compatibility and
		// all other markers (colour profiles, quantisation tables, etc.).
		if marker == 0xE1 {
			stripped = true
			continue
		}
		out.WriteByte(0xFF)
		out.WriteByte(marker)
		out.Write(payload)

		// SOS is followed by raw compressed image data — handled at the top
		// of the loop when data[i] != 0xFF.
	}

	if !stripped {
		return nil, false
	}
	return out.Bytes(), true
}

// ─── Resize ───────────────────────────────────────────────────────────────────

// ScaleDown decodes the image in r and scales it so that neither dimension
// exceeds maxWidth / maxHeight (zero means "no constraint on that axis"). Only
// downscaling is performed; if the image already fits, Result.Reduced is false.
// The resized image is encoded as whichever of JPEG q85 and lossless WebP is
// smaller, so ScaleDown implicitly compresses as well as resizes.
func ScaleDown(r io.ReadSeeker, contentType string, origSize int64, maxWidth, maxHeight int) (Result, error) {
	passthrough := func() (Result, error) {
		if _, err := r.Seek(0, io.SeekStart); err != nil {
			return Result{}, err
		}
		return Result{Body: r, ContentType: contentType, Size: origSize, OriginalSize: origSize}, nil
	}
	if maxWidth <= 0 && maxHeight <= 0 {
		return passthrough()
	}
	if _, err := r.Seek(0, io.SeekStart); err != nil {
		return Result{}, fmt.Errorf("rewind for resize: %w", err)
	}
	src, _, err := image.Decode(r)
	if err != nil {
		return passthrough() // can't decode → skip silently
	}
	scaled := scaleImage(src, maxWidth, maxHeight)
	if scaled == nil {
		return passthrough() // already fits
	}
	return encodeScaled(scaled, origSize), nil
}

// scaleImage returns a downscaled copy of src that fits within maxW×maxH
// (zero = unlimited). Returns nil when no scaling is required.
func scaleImage(src image.Image, maxW, maxH int) image.Image {
	b := src.Bounds()
	w, h := b.Dx(), b.Dy()
	if w == 0 || h == 0 {
		return nil
	}
	newW, newH := w, h
	if maxW > 0 && newW > maxW {
		newH = newH * maxW / newW
		newW = maxW
	}
	if maxH > 0 && newH > maxH {
		newW = newW * maxH / newH
		newH = maxH
	}
	if newW >= w && newH >= h {
		return nil // nothing changed
	}
	if newW < 1 {
		newW = 1
	}
	if newH < 1 {
		newH = 1
	}
	dst := image.NewRGBA(image.Rect(0, 0, newW, newH))
	xdraw.CatmullRom.Scale(dst, dst.Bounds(), src, b, xdraw.Over, nil)
	return dst
}

// encodeScaled encodes img as both JPEG q85 and lossless WebP and returns the
// smaller result. origSize is the pre-resize byte count, used only for the
// OriginalSize field in the returned Result.
func encodeScaled(img image.Image, origSize int64) Result {
	best := Result{ContentType: "", Size: origSize + 1, OriginalSize: origSize}
	var bestBytes []byte

	// JPEG candidate (lossy; skip if image has transparency)
	if !hasTransparency(img) {
		var jbuf bytes.Buffer
		if err := jpeg.Encode(&jbuf, img, &jpeg.Options{Quality: jpegQuality}); err == nil {
			if int64(jbuf.Len()) < best.Size {
				bestBytes = jbuf.Bytes()
				best = Result{ContentType: "image/jpeg", Size: int64(len(bestBytes)), OriginalSize: origSize, Reduced: true}
			}
		}
	}

	// WebP candidate (lossless; preserves transparency)
	var wbuf bytes.Buffer
	if err := nativewebp.Encode(&wbuf, img, &nativewebp.Options{}); err == nil {
		if int64(wbuf.Len()) < best.Size {
			bestBytes = wbuf.Bytes()
			best = Result{ContentType: "image/webp", Size: int64(len(bestBytes)), OriginalSize: origSize, Reduced: true}
		}
	}

	if best.Reduced {
		best.Body = bytes.NewReader(bestBytes)
		return best
	}
	// Both encoders failed or produced larger output — return empty (caller falls back).
	return Result{}
}
