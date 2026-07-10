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

var reference = regexp.MustCompile(`^\$\{([A-Za-z_][A-Za-z0-9_]*)\}$`)

func (Environment) Resolve(s string) (string, error) {
	m := reference.FindStringSubmatch(s)
	if len(m) != 2 {
		return s, nil
	}
	v, ok := os.LookupEnv(m[1])
	if !ok {
		return "", fmt.Errorf("required environment variable %s is not set", m[1])
	}
	return v, nil
}

func (OptionalEnvironment) Resolve(s string) (string, error) {
	m := reference.FindStringSubmatch(s)
	if len(m) != 2 {
		return s, nil
	}
	v, ok := os.LookupEnv(m[1])
	if !ok {
		return s, nil
	}
	return v, nil
}
