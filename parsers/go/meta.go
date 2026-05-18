package synx

// Mode is the file-level parser mode.
type Mode uint8

const (
	ModeStatic Mode = iota
	ModeActive
)

// Constraints from `[min:3, max:30, required, type:int, pattern:..., enum:a|b, readonly]`.
type Constraints struct {
	Min        *float64
	Max        *float64
	TypeName   string // empty when absent
	Required   bool
	Readonly   bool
	Pattern    string // empty when absent
	EnumValues []string
}

func (c Constraints) HasAny() bool {
	return c.Min != nil || c.Max != nil || c.TypeName != "" ||
		c.Required || c.Readonly || c.Pattern != "" || c.EnumValues != nil
}

// Meta is the marker / args / type-hint / constraint bundle attached to one
// active-mode field.
type Meta struct {
	Markers     []string
	Args        []string
	TypeHint    string // empty when absent
	Constraints *Constraints
}

func (m Meta) HasMarker(name string) bool {
	for _, x := range m.Markers {
		if x == name {
			return true
		}
	}
	return false
}

// MarkerIndex returns the index of `name` in the chain, or -1.
func (m Meta) MarkerIndex(name string) int {
	for i, x := range m.Markers {
		if x == name {
			return i
		}
	}
	return -1
}

// MetaMap is `key -> Meta` for one object level.
type MetaMap map[string]Meta

// MetadataTree is keyed by dot-path prefix ("" for the root level).
type MetadataTree map[string]MetaMap

// IncludeDirective parsed from `!include path [alias]`.
type IncludeDirective struct {
	Path  string
	Alias string
}

// UseDirective parsed from `!use @scope/name [as alias]`.
type UseDirective struct {
	Package string
	Alias   string
}

// ParseResult is the complete output of Parse — root tree, mode flags,
// metadata, include/use directives.
type ParseResult struct {
	Root     Value
	Mode     Mode
	Locked   bool
	Tool     bool
	Schema   bool
	Llm      bool
	Metadata MetadataTree
	Includes []IncludeDirective
	Uses     []UseDirective
}
