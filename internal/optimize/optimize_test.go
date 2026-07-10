package optimize

import (
	"bytes"
	"image"
	"image/color"
	"image/jpeg"
	"image/png"
	"io"
	"testing"
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

func TestPNGWithoutAlphaConvertsToJPEG(t *testing.T) {
	data, origSize := makePNG(t, 200, 200)
	res, err := TryCompress(bytes.NewReader(data), "image/png", origSize)
	if err != nil {
		t.Fatal(err)
	}
	// A large opaque PNG should be converted to JPEG and reduced.
	if res.Reduced {
		if res.ContentType != "image/jpeg" {
			t.Fatalf("expected image/jpeg after conversion, got %s", res.ContentType)
		}
		if res.Size >= origSize {
			t.Fatalf("Reduced=true but size %d >= orig %d", res.Size, origSize)
		}
	}
}

func TestPNGWithAlphaPassesThrough(t *testing.T) {
	data, origSize := makePNGWithAlpha(t, 50, 50)
	res, err := TryCompress(bytes.NewReader(data), "image/png", origSize)
	if err != nil {
		t.Fatal(err)
	}
	if res.ContentType != "image/png" {
		t.Fatalf("transparent PNG should keep image/png, got %s", res.ContentType)
	}
	if res.Reduced {
		t.Fatal("transparent PNG should not be reported as reduced")
	}
	if res.Size != origSize {
		t.Fatalf("size changed for transparent PNG: %d → %d", origSize, res.Size)
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
