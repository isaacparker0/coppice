package rust_language

// Metadata about external crates, parsed from Cargo.lock.

import (
	"bufio"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/bazelbuild/bazel-gazelle/config"
)

type ExternalCrates struct {
	nameByImport map[string]string
}

func NewExternalCrates(repoRoot string) *ExternalCrates {
	externalCrates := &ExternalCrates{
		nameByImport: make(map[string]string),
	}

	lockfilePath := filepath.Join(repoRoot, "Cargo.lock")
	if err := externalCrates.parseLockfile(lockfilePath); err != nil {
		return externalCrates
	}

	return externalCrates
}

func (externalCrates *ExternalCrates) GetName(importName string) string {
	normalized := strings.ReplaceAll(importName, "-", "_")

	if name, ok := externalCrates.nameByImport[normalized]; ok {
		return name
	}

	return importName
}

var packageNameRegex = regexp.MustCompile(`^name\s*=\s*"([^"]+)"`)

// Read Cargo.lock and extract package names.
func (externalCrates *ExternalCrates) parseLockfile(path string) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	inPackage := false
	currentPackage := ""

	for scanner.Scan() {
		line := scanner.Text()
		trimmed := strings.TrimSpace(line)

		if trimmed == "[[package]]" {
			if currentPackage != "" {
				normalized := strings.ReplaceAll(currentPackage, "-", "_")
				externalCrates.nameByImport[normalized] = currentPackage
			}

			inPackage = true
			currentPackage = ""
			continue
		}

		if inPackage {
			if matches := packageNameRegex.FindStringSubmatch(trimmed); len(matches) > 1 {
				currentPackage = matches[1]
			}
		}
	}

	if currentPackage != "" {
		normalized := strings.ReplaceAll(currentPackage, "-", "_")
		externalCrates.nameByImport[normalized] = currentPackage
	}

	return scanner.Err()
}

func getExternalCrates(c *config.Config) *ExternalCrates {
	const key = "rust_external_crates"
	if externalCrates, ok := c.Exts[key].(*ExternalCrates); ok {
		return externalCrates
	}
	externalCrates := NewExternalCrates(c.RepoRoot)
	c.Exts[key] = externalCrates
	return externalCrates
}
