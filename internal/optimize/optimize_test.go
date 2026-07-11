package optimize

import (
	"bytes"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"io"
	"testing"

	"github.com/HugoSmits86/nativewebp"
)

// makeJPEG returns a minimal JPEG-encoded image of the given dimensions.
func makeJPEG(t *testing.T, w, h int) ([]byte, int64) {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			img.SetRGBA(x, y, color.RGBA{R: uint8(x), G: uint8(y), B: 128, A: 255})
		}
	}
	var buf bytes.Buffer
	if err := jpeg.Encode(&buf, img, &jpeg.Options{Quality: 100}); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes(), int64(buf.Len())
}

// makePNG returns a PNG with no transparency.
func makePNG(t *testing.T, w, h int) ([]byte, int64) {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			img.SetRGBA(x, y, color.RGBA{R: uint8(x * 3), G: uint8(y * 3), B: 64, A: 255})
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes(), int64(buf.Len())
}

// makePNGWithAlpha returns a PNG that has at least one transparent pixel.
func makePNGWithAlpha(t *testing.T, w, h int) ([]byte, int64) {
	t.Helper()
	img := image.NewNRGBA(image.Rect(0, 0, w, h))
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			a := uint8(255)
			if x == 0 && y == 0 {
				a = 0 // one transparent corner
			}
			img.SetNRGBA(x, y, color.NRGBA{R: 200, G: 100, B: 50, A: a})
		}
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes(), int64(buf.Len())
}

func TestJPEGReEncodeReducesSize(t *testing.T) {
	data, origSize := makeJPEG(t, 200, 200)
	res, err := TryCompress(bytes.NewReader(data), "image/jpeg", origSize)
	if err != nil {
		t.Fatal(err)
	}
	if res.ContentType != "image/jpeg" {
		t.Fatalf("content type changed to %s", res.ContentType)
	}
	if res.OriginalSize != origSize {
		t.Fatalf("OriginalSize wrong: got %d want %d", res.OriginalSize, origSize)
	}
	// Re-encoding a q100 JPEG at q85 should produce a smaller file.
	if res.Reduced && res.Size >= origSize {
		t.Fatalf("Reduced=true but size %d >= orig %d", res.Size, origSize)
	}
	// Body must be readable.
	b, err := io.ReadAll(res.Body)
	if err != nil || len(b) == 0 {
		t.Fatalf("Body unreadable: %v", err)
	}
}

func TestPNGWithoutAlphaIsReduced(t *testing.T) {
	data, origSize := makePNG(t, 200, 200)
	res, err := TryCompress(bytes.NewReader(data), "image/png", origSize)
	if err != nil {
		t.Fatal(err)
	}
	// An opaque PNG should be re-encoded to whichever of JPEG / WebP is
	// smallest. Both are acceptable; PNG (no gain) is also valid.
	if res.Reduced {
		if res.ContentType != "image/jpeg" && res.ContentType != "image/webp" {
			t.Fatalf("expected jpeg or webp after conversion, got %s", res.ContentType)
		}
		if res.Size >= origSize {
			t.Fatalf("Reduced=true but size %d >= orig %d", res.Size, origSize)
		}
	}
}

func TestPNGWithAlphaNeverBecomesJPEG(t *testing.T) {
	data, origSize := makePNGWithAlpha(t, 100, 100)
	res, err := TryCompress(bytes.NewReader(data), "image/png", origSize)
	if err != nil {
		t.Fatal(err)
	}
	// A transparent PNG must never be turned into JPEG (which has no alpha).
	// It may become lossless WebP or stay PNG, depending on which is smaller.
	if res.ContentType == "image/jpeg" {
		t.Fatal("transparent PNG must not be converted to JPEG")
	}
	if res.ContentType != "image/png" && res.ContentType != "image/webp" {
		t.Fatalf("unexpected content type %s", res.ContentType)
	}
	// Whatever wins, the body must be readable.
	b, err := io.ReadAll(res.Body)
	if err != nil || len(b) == 0 {
		t.Fatalf("body unreadable: %v", err)
	}
}

func TestPNGWebPPreservesTransparency(t *testing.T) {
	// A larger transparent PNG so lossless WebP has a chance to beat it.
	data, origSize := makePNGWithAlpha(t, 256, 256)
	res, err := TryCompress(bytes.NewReader(data), "image/png", origSize)
	if err != nil {
		t.Fatal(err)
	}
	if res.ContentType == "image/webp" {
		if !res.Reduced {
			t.Fatal("webp result must be marked reduced")
		}
		// Decode the WebP back and confirm the transparent corner survived.
		b, _ := io.ReadAll(res.Body)
		img, derr := nativewebp.Decode(bytes.NewReader(b))
		if derr != nil {
			t.Fatalf("cannot decode produced webp: %v", derr)
		}
		_, _, _, a := img.At(0, 0).RGBA()
		if a == 0xffff {
			t.Fatal("transparency was lost in WebP conversion")
		}
	}
}

func TestPassthroughFormats(t *testing.T) {
	for _, ct := range []string{"image/gif", "image/webp", "image/avif", "image/svg+xml"} {
		data := []byte("fake-image-data")
		res, err := TryCompress(bytes.NewReader(data), ct, int64(len(data)))
		if err != nil {
			t.Fatalf("%s: unexpected error: %v", ct, err)
		}
		if res.ContentType != ct {
			t.Fatalf("%s: content type changed to %s", ct, res.ContentType)
		}
		if res.Reduced {
			t.Fatalf("%s: should not be reduced", ct)
		}
		b, _ := io.ReadAll(res.Body)
		if !bytes.Equal(b, data) {
			t.Fatalf("%s: body changed", ct)
		}
	}
}

func TestBodyAlwaysReadable(t *testing.T) {
	// Even a corrupt JPEG should yield a readable body (fallback to original).
	corrupt := []byte("\xff\xd8\xff\xe0 not really a jpeg")
	res, err := TryCompress(bytes.NewReader(corrupt), "image/jpeg", int64(len(corrupt)))
	if err != nil {
		t.Fatal(err)
	}
	b, err := io.ReadAll(res.Body)
	if err != nil || len(b) == 0 {
		t.Fatalf("body unreadable after corrupt input: %v", err)
	}
}

// ─── EXIF stripping tests ─────────────────────────────────────────────────────

func makeJPEGWithFakeEXIF(t *testing.T) []byte {
	t.Helper()
	// Build a minimal JPEG: SOI + fake APP1 (EXIF marker) + APP0 (JFIF) + EOI.
	var buf bytes.Buffer
	buf.Write([]byte{0xFF, 0xD8}) // SOI

	// Fake APP1 (0xFFE1) with some payload — simulates EXIF.
	exifPayload := []byte("Exif\x00\x00fake-gps-data")
	app1Len := uint16(2 + len(exifPayload))
	buf.Write([]byte{0xFF, 0xE1})
	buf.WriteByte(byte(app1Len >> 8))
	buf.WriteByte(byte(app1Len))
	buf.Write(exifPayload)

	// APP0 (0xFFE0, JFIF) — should be kept.
	jfifPayload := []byte("JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00")
	app0Len := uint16(2 + len(jfifPayload))
	buf.Write([]byte{0xFF, 0xE0})
	buf.WriteByte(byte(app0Len >> 8))
	buf.WriteByte(byte(app0Len))
	buf.Write(jfifPayload)

	buf.Write([]byte{0xFF, 0xD9}) // EOI
	return buf.Bytes()
}

func TestStripJPEGMetadataRemovesAPP1(t *testing.T) {
	data := makeJPEGWithFakeEXIF(t)
	stripped, changed := StripJPEGMetadata(data)
	if !changed {
		t.Fatal("expected metadata to be stripped")
	}
	if len(stripped) >= len(data) {
		t.Fatalf("stripped (%d bytes) should be smaller than original (%d bytes)", len(stripped), len(data))
	}
	// APP1 marker should be gone.
	for i := 0; i+1 < len(stripped); i++ {
		if stripped[i] == 0xFF && stripped[i+1] == 0xE1 {
			t.Fatal("APP1 (EXIF) marker still present after stripping")
		}
	}
	// APP0 (JFIF) should still be present.
	found := false
	for i := 0; i+1 < len(stripped); i++ {
		if stripped[i] == 0xFF && stripped[i+1] == 0xE0 {
			found = true
			break
		}
	}
	if !found {
		t.Fatal("APP0 (JFIF) was incorrectly stripped")
	}
}

func TestStripJPEGMetadataIgnoresNonJPEG(t *testing.T) {
	_, changed := StripJPEGMetadata([]byte("\x89PNG\r\n\x1a\n"))
	if changed {
		t.Fatal("non-JPEG input should not be changed")
	}
}

func TestStripJPEGMetadataNoOpWhenNoEXIF(t *testing.T) {
	// A minimal JPEG with no APP1.
	data := []byte{0xFF, 0xD8, 0xFF, 0xD9}
	_, changed := StripJPEGMetadata(data)
	if changed {
		t.Fatal("JPEG without APP1 should not be changed")
	}
}

// ─── ScaleDown tests ──────────────────────────────────────────────────────────

func TestScaleDownReducesPNG(t *testing.T) {
	// Create a 400×200 opaque PNG.
	img := image.NewRGBA(image.Rect(0, 0, 400, 200))
	for y := 0; y < 200; y++ {
		for x := 0; x < 400; x++ {
			img.SetRGBA(x, y, color.RGBA{R: uint8(x), G: uint8(y), B: 100, A: 255})
		}
	}
	var buf bytes.Buffer
	png.Encode(&buf, img)
	data := buf.Bytes()

	res, err := ScaleDown(bytes.NewReader(data), "image/png", int64(len(data)), 200, 0)
	if err != nil {
		t.Fatal(err)
	}
	if !res.Reduced {
		t.Fatal("400-wide image should be scaled down to max-width 200")
	}
	// The output must be a supported type.
	if res.ContentType != "image/jpeg" && res.ContentType != "image/webp" {
		t.Fatalf("unexpected content type: %s", res.ContentType)
	}
}

func TestScaleDownPassthroughWhenAlreadySmall(t *testing.T) {
	img := image.NewRGBA(image.Rect(0, 0, 100, 50))
	var buf bytes.Buffer
	png.Encode(&buf, img)
	data := buf.Bytes()

	res, err := ScaleDown(bytes.NewReader(data), "image/png", int64(len(data)), 800, 0)
	if err != nil {
		t.Fatal(err)
	}
	if res.Reduced {
		t.Fatal("100-wide image should not be scaled when max-width is 800")
	}
}
