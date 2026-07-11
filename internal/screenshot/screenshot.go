// Package screenshot captures a screenshot to a temporary file using the
// platform's native screenshot tool.
//
// Supported platforms and tools:
//   - macOS: screencapture (built-in)
//   - Linux: flameshot, scrot, gnome-screenshot (first available wins)
//   - Windows: PowerShell + System.Windows.Forms (full-screen only)
package screenshot

import (
	"fmt"
	"os"
	"os/exec"
	"runtime"
)

// Mode controls what region of the screen is captured.
type Mode int

const (
	// ModeFullScreen captures the entire primary display (default).
	ModeFullScreen Mode = iota
	// ModeRegion lets the user interactively draw a selection rectangle.
	ModeRegion
	// ModeWindow captures the currently active window.
	ModeWindow
)

// Capture takes a screenshot and writes it to a new temporary PNG file.
// The caller is responsible for removing the file when done.
func Capture(m Mode) (path string, err error) {
	f, err := os.CreateTemp("", "img-screenshot-*.png")
	if err != nil {
		return "", fmt.Errorf("create screenshot temp file: %w", err)
	}
	path = f.Name()
	f.Close()

	if err := capture(path, m); err != nil {
		os.Remove(path)
		return "", err
	}
	// Verify the tool actually wrote something.
	if fi, err := os.Stat(path); err != nil || fi.Size() == 0 {
		os.Remove(path)
		return "", fmt.Errorf("screenshot cancelled or produced an empty file")
	}
	return path, nil
}

func capture(path string, m Mode) error {
	switch runtime.GOOS {
	case "darwin":
		return captureDarwin(path, m)
	case "linux":
		return captureLinux(path, m)
	case "windows":
		return captureWindows(path, m)
	default:
		return fmt.Errorf("screenshot is not supported on %s", runtime.GOOS)
	}
}

// ─── macOS ────────────────────────────────────────────────────────────────────

func captureDarwin(path string, m Mode) error {
	// -x  suppress camera shutter sound
	args := []string{"-x"}
	switch m {
	case ModeRegion:
		args = append(args, "-i") // interactive crosshair selection
	case ModeWindow:
		args = append(args, "-w") // click to select a window
	}
	args = append(args, path)
	out, err := exec.Command("screencapture", args...).CombinedOutput()
	if err != nil {
		return fmt.Errorf("screencapture: %w\n%s", err, out)
	}
	return nil
}

// ─── Linux ────────────────────────────────────────────────────────────────────

func captureLinux(path string, m Mode) error {
	// Try tools in order of preference.
	type tool struct {
		bin  string
		args func(Mode, string) []string
	}
	tools := []tool{
		{
			"flameshot",
			func(m Mode, p string) []string {
				switch m {
				case ModeRegion:
					// gui --raw outputs PNG to stdout; --path writes to file.
					return []string{"gui", "--path", p}
				default:
					return []string{"full", "--path", p}
				}
			},
		},
		{
			"scrot",
			func(m Mode, p string) []string {
				switch m {
				case ModeRegion:
					return []string{"-s", p} // -s interactive selection
				case ModeWindow:
					return []string{"-u", p} // -u focused window
				default:
					return []string{p}
				}
			},
		},
		{
			"gnome-screenshot",
			func(m Mode, p string) []string {
				switch m {
				case ModeRegion:
					return []string{"-a", "-f", p} // -a area selection
				case ModeWindow:
					return []string{"-w", "-f", p}
				default:
					return []string{"-f", p}
				}
			},
		},
		{
			"import", // part of ImageMagick
			func(m Mode, p string) []string {
				switch m {
				case ModeRegion, ModeWindow:
					return []string{p} // import without -window takes interactive selection
				default:
					return []string{"-window", "root", p}
				}
			},
		},
	}

	for _, t := range tools {
		if _, err := exec.LookPath(t.bin); err != nil {
			continue // tool not installed
		}
		args := t.args(m, path)
		out, err := exec.Command(t.bin, args...).CombinedOutput()
		if err != nil {
			return fmt.Errorf("%s: %w\n%s", t.bin, err, out)
		}
		return nil
	}
	return fmt.Errorf("no screenshot tool found; install flameshot, scrot, gnome-screenshot, or ImageMagick")
}

// ─── Windows ──────────────────────────────────────────────────────────────────

func captureWindows(path string, m Mode) error {
	if m != ModeFullScreen {
		return fmt.Errorf("--region and --window are not supported on Windows; use full-screen capture")
	}
	// Use PowerShell to capture the primary screen to a PNG file.
	script := `
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
$s = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$bmp = New-Object System.Drawing.Bitmap($s.Width, $s.Height, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.CopyFromScreen($s.Location, [System.Drawing.Point]::Empty, $s.Size)
$g.Dispose()
$bmp.Save('` + path + `', [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
`
	out, err := exec.Command("powershell", "-NoProfile", "-NonInteractive", "-Command", script).CombinedOutput()
	if err != nil {
		return fmt.Errorf("powershell screenshot: %w\n%s", err, out)
	}
	return nil
}
