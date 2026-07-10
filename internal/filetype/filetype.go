package filetype

import (
	"fmt"
	"mime"
	"net/http"
	"os"
	"path/filepath"
	"strings"
)

var allowed = map[string]bool{"image/png": true, "image/jpeg": true, "image/gif": true, "image/webp": true, "image/svg+xml": true, "image/avif": true}

func Inspect(path string, maxSize int64) (string, int64, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", 0, fmt.Errorf("open image %s: %w", path, err)
	}
	defer f.Close()
	st, err := f.Stat()
	if err != nil {
		return "", 0, fmt.Errorf("stat image %s: %w", path, err)
	}
	if !st.Mode().IsRegular() {
		return "", 0, fmt.Errorf("image %s is not a regular file", path)
	}
	if st.Size() == 0 {
		return "", 0, fmt.Errorf("image %s is empty", path)
	}
	if st.Size() > maxSize {
		return "", 0, fmt.Errorf("image %s exceeds maximum size of %d bytes", path, maxSize)
	}
	b := make([]byte, 512)
	n, _ := f.Read(b)
	typ := strings.Split(http.DetectContentType(b[:n]), ";")[0]
	if !allowed[typ] {
		extTyp := strings.Split(mime.TypeByExtension(strings.ToLower(filepath.Ext(path))), ";")[0]
		if allowed[extTyp] {
			typ = extTyp
		}
	}
	if !allowed[typ] {
		return "", 0, fmt.Errorf("file %s is not a supported image (detected %s)", path, typ)
	}
	return typ, st.Size(), nil
}
