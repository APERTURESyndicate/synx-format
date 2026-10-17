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

	// `[type:]` / `[pattern:]` declare an empty type or pattern, which is not
	// the same as declaring none: the first declaration of a field wins.
	typeNameSet bool
	patternSet  bool
}

func (c Constraints) HasAny() bool {
	return c.Min != nil || c.Max != nil || c.hasTypeName() ||
		c.Required || c.Readonly || c.hasPattern() || c.EnumValues != nil
}

func (c *Constraints) hasTypeName() bool { return c.TypeName != "" || c.typeNameSet }
func (c *Constraints) hasPattern() bool  { return c.Pattern != "" || c.patternSet }

// Meta is the marker / args / type-hint / constraint bundle attached to one
// active-mode field.
type Meta struct {
	Markers     []string
	Args        []string
	TypeHint    string // empty when absent
	Constraints *Constraints

	// `key() value` declares an empty type hint (see Constraints.typeNameSet).
	typeHintSet bool
}

func (m *Meta) hasTypeHint() bool { return m.TypeHint != "" || m.typeHintSet }

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

// MetaMap is `key -> Meta` for one object, in declaration order.
type MetaMap struct {
	keys  []string
	items map[string]*Meta
}

// NewMetaMap returns an empty MetaMap.
func NewMetaMap() *MetaMap {
	return &MetaMap{items: map[string]*Meta{}}
}

// Get returns the declaration of `key`, or (nil, false).
func (m *MetaMap) Get(key string) (*Meta, bool) {
	if m == nil {
		return nil, false
	}
	meta, ok := m.items[key]
	return meta, ok
}

// Set stores a declaration; a key declared again keeps its first position.
func (m *MetaMap) Set(key string, meta Meta) {
	if m.items == nil {
		m.items = map[string]*Meta{}
	}
	if _, ok := m.items[key]; !ok {
		m.keys = append(m.keys, key)
	}
	cp := meta
	m.items[key] = &cp
}

// Keys returns the declared keys in order.
func (m *MetaMap) Keys() []string {
	if m == nil {
		return nil
	}
	return append([]string(nil), m.keys...)
}

// Len returns the number of declared keys.
func (m *MetaMap) Len() int {
	if m == nil {
		return 0
	}
	return len(m.keys)
}

// Clone copies the map; the declarations themselves are shared values.
func (m *MetaMap) Clone() *MetaMap {
	out := &MetaMap{keys: append([]string(nil), m.keys...), items: make(map[string]*Meta, len(m.items))}
	for k, v := range m.items {
		cp := *v
		out.items[k] = &cp
	}
	return out
}

// MetadataTree maps an object's metadata path to its declarations, in the
// order the paths were first declared. The root object is "", a nested object
// is its keys joined with '.', a list item adds `#<index>` (`servers.#0`),
// and '.' or '\' inside a key is escaped with '\' (`a\.b`).
type MetadataTree struct {
	paths  []string
	levels map[string]*MetaMap
}

// NewMetadataTree returns an empty tree.
func NewMetadataTree() *MetadataTree {
	return &MetadataTree{levels: map[string]*MetaMap{}}
}

// Get returns the declarations of the object at `path`, or nil.
func (t *MetadataTree) Get(path string) *MetaMap {
	if t == nil {
		return nil
	}
	return t.levels[path]
}

// Set replaces the declarations at `path`; a new path is appended.
func (t *MetadataTree) Set(path string, m *MetaMap) {
	if t.levels == nil {
		t.levels = map[string]*MetaMap{}
	}
	if _, ok := t.levels[path]; !ok {
		t.paths = append(t.paths, path)
	}
	t.levels[path] = m
}

// level returns the declarations at `path`, creating them when absent.
func (t *MetadataTree) level(path string) *MetaMap {
	if m, ok := t.levels[path]; ok {
		return m
	}
	m := NewMetaMap()
	t.Set(path, m)
	return m
}

// Remove drops the declarations at `path`, keeping the order of the rest.
func (t *MetadataTree) Remove(path string) bool {
	if t == nil {
		return false
	}
	if _, ok := t.levels[path]; !ok {
		return false
	}
	t.retain(func(p string) bool { return p != path })
	return true
}

func (t *MetadataTree) retain(keep func(path string) bool) {
	n := 0
	for _, p := range t.paths {
		if keep(p) {
			t.paths[n] = p
			n++
		} else {
			delete(t.levels, p)
		}
	}
	t.paths = t.paths[:n]
}

// Paths returns the declared paths in order.
func (t *MetadataTree) Paths() []string {
	if t == nil {
		return nil
	}
	return append([]string(nil), t.paths...)
}

// Len returns the number of paths.
func (t *MetadataTree) Len() int {
	if t == nil {
		return 0
	}
	return len(t.paths)
}

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
	Metadata *MetadataTree
	Includes []IncludeDirective
	Uses     []UseDirective
	// Truncated reports that a resource limit dropped input: the total byte
	// cap, the line cap, or a multiline block body cap.
	Truncated bool
}
