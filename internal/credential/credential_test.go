package credential

import (
	"strings"
	"testing"
)

func TestResolveSingleVar(t *testing.T) {
	t.Setenv("CRED_TEST_VAR", "hello")
	got, err := Environment{}.Resolve("${CRED_TEST_VAR}")
	if err != nil || got != "hello" {
		t.Fatalf("got %q err %v", got, err)
	}
}

func TestResolveMultipleVars(t *testing.T) {
	t.Setenv("CRED_A", "foo")
	t.Setenv("CRED_B", "bar")
	got, err := Environment{}.Resolve("${CRED_A}:${CRED_B}")
	if err != nil || got != "foo:bar" {
		t.Fatalf("got %q err %v", got, err)
	}
}

func TestReportsAllMissingVars(t *testing.T) {
	// Ensure vars are unset (t.Setenv can't unset, use direct unset via not setting)
	_, err := Environment{}.Resolve("${CRED_MISSING_X_ZZZ}:${CRED_MISSING_Y_ZZZ}")
	if err == nil {
		t.Fatal("expected error for missing vars")
	}
	if !strings.Contains(err.Error(), "CRED_MISSING_X_ZZZ") || !strings.Contains(err.Error(), "CRED_MISSING_Y_ZZZ") {
		t.Fatalf("both missing vars should appear in error: %v", err)
	}
}

func TestReportsSingleMissingVar(t *testing.T) {
	_, err := Environment{}.Resolve("${CRED_ONLY_MISSING_ZZZ}")
	if err == nil {
		t.Fatal("expected error")
	}
	if !strings.Contains(err.Error(), "CRED_ONLY_MISSING_ZZZ") {
		t.Fatalf("missing var should appear in error: %v", err)
	}
}

func TestPassThroughLiteralString(t *testing.T) {
	got, err := Environment{}.Resolve("no-vars-here")
	if err != nil || got != "no-vars-here" {
		t.Fatalf("got %q err %v", got, err)
	}
}

func TestIsReference(t *testing.T) {
	cases := []struct {
		s  string
		ok bool
	}{
		{"${FOO}", true},
		{"${FOO_BAR_123}", true},
		{"prefix-${VAR}-suffix", true},
		{"plaintext", false},
		{"$VAR", false},
		{"{VAR}", false},
		{"", false},
	}
	for _, tc := range cases {
		if IsReference(tc.s) != tc.ok {
			t.Errorf("IsReference(%q) = %v, want %v", tc.s, !tc.ok, tc.ok)
		}
	}
}
