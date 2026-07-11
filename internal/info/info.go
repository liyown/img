// Package info inspects local image files and reports their type, dimensions,
// file size, and whether EXIF metadata is present.
package info

import (
	"encoding/json"
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	_ "image/png"
	"io"
	"os"
	"strings"

	"github.com/liyown/img/internal/filetype"
)

// ImageInfo holds the result of inspecting a single image file.
type ImageInfo struct {
	Path        string `json:"path"`
	ContentType string `json:"content_type,omitempty"`
	Width       int    `json:"width,omitempty"`
	Height      int    `json:"height,omitempty"`
	Size        int64  `json:"size"`
	HasEXIF     bool   `json:"has_exif,omitempty"` // JPEG only
	Error       string `json:"error,omitempty"`
}

// Inspect returns metadata about the image at path. It never returns a Go
// error; any problem is recorded in ImageInfo.Error instead.
func Inspect(path string) ImageInfo {
	info := ImageInfo{Path: path}

	fi, err := os.Stat(path)
	if err != nil {
		info.Error = err.Error()
		return info
	}
	info.Size = fi.Size()

	f, err := os.Open(path)
	if err != nil {
		info.Error = err.Error()
		return info
	}
	defer f.Close()

	ct, _, err := filetype.InspectFile(f, path, info.Size+1)
	if err != nil {
		info.Error = err.Error()
		return info
	}
	info.ContentType = ct

	// Dimensions — use DecodeConfig which reads only the image header.
	if _, err := f.Seek(0, io.SeekStart); err == nil {
		if cfg, _, err := image.DecodeConfig(f); err == nil {
			info.Width = cfg.Width
			info.Height = cfg.Height
		}
	}

	// EXIF detection: scan JPEG bytes for APP1 (0xFFE1) marker.
	if ct == "image/jpeg" {
		if _, err := f.Seek(0, io.SeekStart); err == nil {
			info.HasEXIF = hasAPP1(f)
		}
	}

	return info
}

// hasAPP1 scans the first 64 KB of a JPEG stream for an APP1 marker (0xFFE1),
// which is used by EXIF and XMP metadata.
func hasAPP1(r io.Reader) bool {
	buf := make([]byte, 65536)
	n, _ := io.ReadFull(r, buf)
	data := buf[:n]
	for i := 0; i+1 < len(data); i++ {
		if data[i] == 0xFF && data[i+1] == 0xE1 {
			return true
		}
	}
	return false
}

// Format returns a human-readable one-line description of info, suitable for
// terminal output.
func (i ImageInfo) Format() string {
	if i.Error != "" {
		return fmt.Sprintf("%-40s  %-10s  %s", truncatePath(i.Path, 40), "error", i.Error)
	}
	typShort := shortType(i.ContentType)
	dims := "–"
	if i.Width > 0 && i.Height > 0 {
		dims = fmt.Sprintf("%d×%d", i.Width, i.Height)
	}
	sizeStr := formatBytes(i.Size)
	exif := ""
	if i.HasEXIF {
		exif = "  ⚠ EXIF"
	}
	return fmt.Sprintf("%-40s  %-6s  %-12s  %s%s",
		truncatePath(i.Path, 40), typShort, dims, sizeStr, exif)
}

// PrintJSON writes the JSON representation of a slice of ImageInfo to w.
func PrintJSON(w io.Writer, infos []ImageInfo) error {
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	return enc.Encode(infos)
}

func shortType(ct string) string {
	switch ct {
	case "image/jpeg":
		return "JPEG"
	case "image/png":
		return "PNG"
	case "image/gif":
		return "GIF"
	case "image/webp":
		return "WebP"
	case "image/avif":
		return "AVIF"
	case "image/svg+xml":
		return "SVG"
	default:
		if ct != "" {
			return strings.TrimPrefix(ct, "image/")
		}
		return "?"
	}
}

func formatBytes(n int64) string {
	switch {
	case n >= 1<<20:
		return fmt.Sprintf("%.1f MB", float64(n)/(1<<20))
	case n >= 1<<10:
		return fmt.Sprintf("%.0f KB", float64(n)/(1<<10))
	default:
		return fmt.Sprintf("%d B", n)
	}
}

func truncatePath(s string, max int) string {
	if len(s) <= max {
		return s
	}
	return "…" + s[len(s)-max+1:]
}
