package credential

import (
	"fmt"
	"os"
	"regexp"
	"strings"
)

// Resolver isolates credential lookup from providers and configuration storage.
// Keychain, Credential Manager, and Secret Service implementations can satisfy it.
type Resolver interface{ Resolve(string) (string, error) }
type Environment struct{}

var reference = regexp.MustCompile(`\$\{([A-Za-z_][A-Za-z0-9_]*)\}`)

func IsReference(s string) bool { return reference.MatchString(s) }

func (Environment) Resolve(s string) (string, error) {
	var missing []string
	result := reference.ReplaceAllStringFunc(s, func(match string) string {
		m := reference.FindStringSubmatch(match)
		v, ok := os.LookupEnv(m[1])
		if !ok {
			missing = append(missing, m[1])
			return match
		}
		return v
	})
	if len(missing) > 0 {
		if len(missing) == 1 {
			return "", fmt.Errorf("required environment variable %s is not set", missing[0])
		}
		return "", fmt.Errorf("required environment variables are not set: %s", strings.Join(missing, ", "))
	}
	return result, nil
}
