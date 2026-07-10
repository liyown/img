package filetype

import (
	"bytes"
	"fmt"
	"io"
	"net/http"
	"os"
	"strings"
)

var allowed = map[string]bool{"image/png": true, "image/jpeg": true, "image/gif": true, "image/webp": true, "image/svg+xml": true, "image/avif": true}

func Inspect(path string, maxSize int64) (string, int64, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", 0, fmt.Errorf("open image %s: %w", path, err)
	}
	defer f.Close()
	return InspectFile(f, path, maxSize)
}

func InspectFile(f *os.File, displayPath string, maxSize int64) (string, int64, error) {
	st, err := f.Stat()
	if err != nil {
		return "", 0, fmt.Errorf("stat image %s: %w", displayPath, err)
	}
	if !st.Mode().IsRegular() {
		return "", 0, fmt.Errorf("image %s is not a regular file", displayPath)
	}
	if st.Size() == 0 {
		return "", 0, fmt.Errorf("image %s is empty", displayPath)
	}
	if st.Size() > maxSize {
		return "", 0, fmt.Errorf("image %s exceeds maximum size of %d bytes", displayPath, maxSize)
	}

	// 512 bytes suffices for net/http.DetectContentType and most binary formats.
	// SVG files can have a long <?xml ... ?> declaration before the <svg tag, so
	// we read up to 4 KiB to give isSVG enough context.
	b := make([]byte, 4096)
	n, err := io.ReadFull(f, b)
	if err != nil && err != io.ErrUnexpectedEOF {
		return "", 0, fmt.Errorf("read image header %s: %w", displayPath, err)
	}
	if _, err := f.Seek(0, io.SeekStart); err != nil {
		return "", 0, fmt.Errorf("rewind image %s: %w", displayPath, err)
	}

	typ := strings.Split(http.DetectContentType(b[:n]), ";")[0]
	if isSVG(b[:n]) {
		typ = "image/svg+xml"
	}
	if isAVIF(b[:n]) {
		typ = "image/avif"
	}
	if !allowed[typ] {
		return "", 0, fmt.Errorf("file %s is not a supported image (detected %s)", displayPath, typ)
	}
	return typ, st.Size(), nil
}

func isSVG(b []byte) bool {
	b = bytes.TrimPrefix(b, []byte{0xef, 0xbb, 0xbf})
	s := strings.ToLower(strings.TrimSpace(string(b)))
	if strings.HasPrefix(s, "<?xml") {
		if end := strings.Index(s, "?>"); end >= 0 {
			s = strings.TrimSpace(s[end+2:])
		}
	}
	return strings.HasPrefix(s, "<svg")
}

func isAVIF(b []byte) bool {
	if len(b) < 16 || string(b[4:8]) != "ftyp" {
		return false
	}
	for i := 8; i+4 <= len(b) && i < 32; i += 4 {
		brand := string(b[i : i+4])
		if brand == "avif" || brand == "avis" {
			return true
		}
	}
	return false
}
