package credential

import (
	"fmt"
	"os"
	"regexp"
)

// Resolver isolates credential lookup from providers and configuration storage.
// Keychain, Credential Manager, and Secret Service implementations can satisfy it.
type Resolver interface{ Resolve(string) (string, error) }
type Environment struct{}
type OptionalEnvironment struct{}

var reference = regexp.MustCompile(`\$\{([A-Za-z_][A-Za-z0-9_]*)\}`)

func (Environment) Resolve(s string) (string, error) {
	var resolveErr error
	result := reference.ReplaceAllStringFunc(s, func(match string) string {
		m := reference.FindStringSubmatch(match)
		v, ok := os.LookupEnv(m[1])
		if !ok {
			resolveErr = fmt.Errorf("required environment variable %s is not set", m[1])
			return match
		}
		return v
	})
	if resolveErr != nil {
		return "", resolveErr
	}
	return result, nil
}

func (OptionalEnvironment) Resolve(s string) (string, error) {
	return reference.ReplaceAllStringFunc(s, func(match string) string {
		m := reference.FindStringSubmatch(match)
		if v, ok := os.LookupEnv(m[1]); ok {
			return v
		}
		return match
	}), nil
}
