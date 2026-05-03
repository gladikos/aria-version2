export interface MockFolder {
  id:        string
  name:      string
  chatCount: number
}

export const MOCK_FOLDERS: MockFolder[] = [
  { id: 'f1', name: 'Code',     chatCount: 3 },
  { id: 'f2', name: 'Personal', chatCount: 2 },
  { id: 'f3', name: 'Research', chatCount: 4 },
]
