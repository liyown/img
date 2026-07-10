package clipboard

import (
	"fmt"
	"os/exec"
	"runtime"
)

type Clipboard interface{ Write(string) error }
type System struct{}

func (System) Write(s string) error {
	var c *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		c = exec.Command("pbcopy")
	case "windows":
		c = exec.Command("clip")
	default:
		c = exec.Command("xclip", "-selection", "clipboard")
	}
	in, e := c.StdinPipe()
	if e != nil {
		return fmt.Errorf("open clipboard input: %w", e)
	}
	if e = c.Start(); e != nil {
		return fmt.Errorf("start clipboard helper: %w", e)
	}
	if _, e = in.Write([]byte(s)); e != nil {
		return fmt.Errorf("write clipboard: %w", e)
	}
	_ = in.Close()
	if e = c.Wait(); e != nil {
		return fmt.Errorf("clipboard helper: %w", e)
	}
	return nil
}
