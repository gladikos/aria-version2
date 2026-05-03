import Database from '@tauri-apps/plugin-sql'

// ─── Public types ──────────────────────────────────────────────────────────────

export interface Folder {
  id:        string
  name:      string
  createdAt: number
  chatCount: number
}

export interface Chat {
  id:           string
  title:        string | null
  folderId:     string | null
  createdAt:    number
  updatedAt:    number
  lastMessage:  string | null
  messageCount: number
}

export interface Message {
  id:        string
  chatId:    string
  role:      'user' | 'assistant'
  content:   string
  createdAt: number
}

// ─── Raw DB row shapes (snake_case from SQLite) ────────────────────────────────

interface FolderRow  { id: string; name: string; created_at: number; chat_count: number }
interface ChatRow    { id: string; title: string | null; folder_id: string | null; created_at: number; updated_at: number; last_message: string | null; message_count: number }
interface MessageRow { id: string; chat_id: string; role: string; content: string; created_at: number }

// ─── Connection ────────────────────────────────────────────────────────────────

let _db: Database | null = null

async function getDb(): Promise<Database> {
  if (!_db) {
    _db = await Database.load('sqlite:aria.db')
    await _db.execute('PRAGMA foreign_keys = ON')
  }
  return _db
}

// ─── Init / migrations ─────────────────────────────────────────────────────────

export async function initDb(): Promise<void> {
  const db = await getDb()

  await db.execute(`
    CREATE TABLE IF NOT EXISTS folders (
      id         TEXT    PRIMARY KEY,
      name       TEXT    NOT NULL,
      created_at INTEGER NOT NULL
    )
  `)

  await db.execute(`
    CREATE TABLE IF NOT EXISTS chats (
      id         TEXT    PRIMARY KEY,
      title      TEXT,
      folder_id  TEXT    REFERENCES folders(id) ON DELETE SET NULL,
      created_at INTEGER NOT NULL,
      updated_at INTEGER NOT NULL
    )
  `)

  await db.execute(`
    CREATE TABLE IF NOT EXISTS messages (
      id         TEXT    PRIMARY KEY,
      chat_id    TEXT    NOT NULL REFERENCES chats(id) ON DELETE CASCADE,
      role       TEXT    NOT NULL,
      content    TEXT    NOT NULL,
      created_at INTEGER NOT NULL
    )
  `)

  await db.execute(`CREATE INDEX IF NOT EXISTS idx_chats_folder   ON chats(folder_id)`)
  await db.execute(`CREATE INDEX IF NOT EXISTS idx_chats_updated  ON chats(updated_at DESC)`)
  await db.execute(`CREATE INDEX IF NOT EXISTS idx_messages_chat  ON messages(chat_id, created_at)`)

  // Seed default folders on first run
  const existing = await db.select<{ id: string }[]>('SELECT id FROM folders LIMIT 1')
  if (existing.length === 0) {
    const now = Date.now()
    for (const [i, name] of ['Code', 'Personal', 'Research'].entries()) {
      await db.execute(
        'INSERT INTO folders (id, name, created_at) VALUES ($1, $2, $3)',
        [`f-${name.toLowerCase()}`, name, now + i],
      )
    }
  }
}

// ─── Folders ───────────────────────────────────────────────────────────────────

export async function listFolders(): Promise<Folder[]> {
  const db = await getDb()
  const rows = await db.select<FolderRow[]>(`
    SELECT f.id, f.name, f.created_at, COUNT(c.id) AS chat_count
    FROM   folders f
    LEFT   JOIN chats c ON c.folder_id = f.id
    GROUP  BY f.id
    ORDER  BY f.created_at ASC
  `)
  return rows.map(r => ({
    id: r.id, name: r.name, createdAt: r.created_at, chatCount: Number(r.chat_count),
  }))
}

export async function createFolder(name: string): Promise<Folder> {
  const db  = await getDb()
  const id  = crypto.randomUUID()
  const now = Date.now()
  await db.execute('INSERT INTO folders (id, name, created_at) VALUES ($1, $2, $3)', [id, name, now])
  return { id, name, createdAt: now, chatCount: 0 }
}

export async function renameFolder(id: string, name: string): Promise<void> {
  const db = await getDb()
  await db.execute('UPDATE folders SET name = $1 WHERE id = $2', [name, id])
}

export async function deleteFolder(id: string): Promise<void> {
  const db = await getDb()
  await db.execute('DELETE FROM folders WHERE id = $1', [id])
}

// ─── Chats ─────────────────────────────────────────────────────────────────────

const CHAT_SELECT = `
  SELECT
    c.id, c.title, c.folder_id, c.created_at, c.updated_at,
    SUBSTR(
      (SELECT content FROM messages WHERE chat_id = c.id ORDER BY created_at DESC LIMIT 1),
      1, 120
    ) AS last_message,
    (SELECT COUNT(*) FROM messages WHERE chat_id = c.id) AS message_count
  FROM chats c
`

function rowToChat(r: ChatRow): Chat {
  return {
    id:           r.id,
    title:        r.title,
    folderId:     r.folder_id,
    createdAt:    r.created_at,
    updatedAt:    r.updated_at,
    lastMessage:  r.last_message,
    messageCount: Number(r.message_count),
  }
}

export async function listChats(folderId?: string | null): Promise<Chat[]> {
  const db = await getDb()
  let   sql    = CHAT_SELECT
  const params: unknown[] = []

  if (folderId === null) {
    sql += ' WHERE c.folder_id IS NULL'
  } else if (folderId !== undefined) {
    sql += ' WHERE c.folder_id = $1'
    params.push(folderId)
  }
  sql += ' ORDER BY c.updated_at DESC'

  const rows = await db.select<ChatRow[]>(sql, params)
  return rows.map(rowToChat)
}

export async function getChat(id: string): Promise<Chat | null> {
  const db   = await getDb()
  const rows = await db.select<ChatRow[]>(CHAT_SELECT + ' WHERE c.id = $1', [id])
  return rows.length ? rowToChat(rows[0]) : null
}

export async function createChat(folderId?: string | null): Promise<Chat> {
  const db  = await getDb()
  const id  = crypto.randomUUID()
  const now = Date.now()
  await db.execute(
    'INSERT INTO chats (id, title, folder_id, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)',
    [id, null, folderId ?? null, now, now],
  )
  return { id, title: null, folderId: folderId ?? null, createdAt: now, updatedAt: now, lastMessage: null, messageCount: 0 }
}

export async function renameChat(id: string, title: string): Promise<void> {
  const db = await getDb()
  await db.execute('UPDATE chats SET title = $1 WHERE id = $2', [title, id])
}

export async function moveChat(id: string, folderId: string | null): Promise<void> {
  const db = await getDb()
  await db.execute('UPDATE chats SET folder_id = $1 WHERE id = $2', [folderId, id])
}

export async function deleteChat(id: string): Promise<void> {
  const db = await getDb()
  await db.execute('DELETE FROM chats WHERE id = $1', [id])
}

export async function touchChat(id: string): Promise<void> {
  const db = await getDb()
  await db.execute('UPDATE chats SET updated_at = $1 WHERE id = $2', [Date.now(), id])
}

// ─── Messages ──────────────────────────────────────────────────────────────────

export async function listMessages(chatId: string): Promise<Message[]> {
  const db   = await getDb()
  const rows = await db.select<MessageRow[]>(
    'SELECT id, chat_id, role, content, created_at FROM messages WHERE chat_id = $1 ORDER BY created_at ASC',
    [chatId],
  )
  return rows.map(r => ({
    id: r.id, chatId: r.chat_id,
    role: r.role as 'user' | 'assistant',
    content: r.content, createdAt: r.created_at,
  }))
}

export async function appendMessage(
  chatId:  string,
  role:    'user' | 'assistant',
  content: string,
): Promise<Message> {
  const db  = await getDb()
  const id  = crypto.randomUUID()
  const now = Date.now()
  await db.execute(
    'INSERT INTO messages (id, chat_id, role, content, created_at) VALUES ($1, $2, $3, $4, $5)',
    [id, chatId, role, content, now],
  )
  return { id, chatId, role, content, createdAt: now }
}
