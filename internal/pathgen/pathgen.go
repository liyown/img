package pathgen

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strings"
	"time"
)

func Generate(local, template, prefix, rename string, now time.Time) (string, error) {
	f, err := os.Open(local)
	if err != nil {
		return "", fmt.Errorf("open %s for path generation: %w", local, err)
	}
	defer f.Close()
	return GenerateFromReader(local, f, template, prefix, rename, now)
}

func GenerateFromReader(local string, reader io.ReadSeeker, template, prefix, rename string, now time.Time) (string, error) {
	base := filepath.Base(local)
	ext := strings.ToLower(filepath.Ext(base))
	stem := strings.TrimSuffix(base, filepath.Ext(base))
	hash, err := readerHash(reader)
	if err != nil {
		return "", err
	}
	id, err := uuid()
	if err != nil {
		return "", err
	}
	name := base
	switch rename {
	case "original", "":
	case "timestamp":
		name = fmt.Sprintf("%d%s", now.Unix(), ext)
	case "hash":
		name = hash + ext
	case "uuid":
		name = id + ext
	default:
		return "", fmt.Errorf("unknown rename strategy %q", rename)
	}
	vars := map[string]string{"{year}": now.Format("2006"), "{month}": now.Format("01"), "{day}": now.Format("02"), "{timestamp}": now.Format("20060102-150405"), "{unix}": fmt.Sprint(now.Unix()), "{filename}": name, "{stem}": stem, "{ext}": strings.TrimPrefix(ext, "."), "{hash}": hash, "{uuid}": id}
	out := template
	for k, v := range vars {
		out = strings.ReplaceAll(out, k, v)
	}
	if prefix != "" {
		out = path.Join(strings.ReplaceAll(prefix, "\\", "/"), out)
	}
	return Validate(strings.ReplaceAll(out, "\\", "/"))
}

func Validate(p string) (string, error) {
	if p == "" || strings.HasPrefix(p, "/") || path.IsAbs(p) {
		return "", fmt.Errorf("remote path must be relative and non-empty")
	}
	for _, s := range strings.Split(p, "/") {
		if s == "" || s == "." || s == ".." {
			return "", fmt.Errorf("remote path contains invalid segment %q", s)
		}
		for _, r := range s {
			if r < 32 || r == 127 {
				return "", fmt.Errorf("remote path contains a control character")
			}
		}
	}
	clean := path.Clean(p)
	if strings.HasPrefix(clean, "../") {
		return "", fmt.Errorf("remote path escapes its root")
	}
	return clean, nil
}
func EscapeURLPath(p string) string {
	parts := strings.Split(p, "/")
	for i := range parts {
		parts[i] = pathEscape(parts[i])
	}
	return strings.Join(parts, "/")
}
func pathEscape(s string) string { // url.PathEscape keeps slashes escaped per segment.
	const hexchars = "0123456789ABCDEF"
	var b strings.Builder
	for _, c := range []byte(s) {
		if (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || strings.ContainsRune("-._~", rune(c)) {
			b.WriteByte(c)
		} else {
			b.WriteByte('%')
			b.WriteByte(hexchars[c>>4])
			b.WriteByte(hexchars[c&15])
		}
	}
	return b.String()
}
func readerHash(f io.ReadSeeker) (string, error) {
	if _, e := f.Seek(0, io.SeekStart); e != nil {
		return "", fmt.Errorf("rewind file for hashing: %w", e)
	}
	h := sha256.New()
	if _, e := io.Copy(h, f); e != nil {
		return "", fmt.Errorf("hash file: %w", e)
	}
	if _, e := f.Seek(0, io.SeekStart); e != nil {
		return "", fmt.Errorf("rewind file after hashing: %w", e)
	}
	return hex.EncodeToString(h.Sum(nil))[:32], nil
}
func uuid() (string, error) {
	b := make([]byte, 16)
	if _, e := rand.Read(b); e != nil {
		return "", fmt.Errorf("generate uuid: %w", e)
	}
	b[6] = b[6]&0x0f | 0x40
	b[8] = b[8]&0x3f | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[:4], b[4:6], b[6:8], b[8:10], b[10:]), nil
}
