import { useEffect, useState } from "react"
import {
  FolderOpen,
  Plus,
  Clock,
  X,
  CircleHelp,
  ArrowRight,
  Cloud,
} from "lucide-react"
import { Button } from "@/components/ui/button"
import {
  getRecentProjects,
  removeFromRecentProjects,
} from "@/lib/project-store"
import type { WikiProject } from "@/types/wiki"
import { useTranslation } from "react-i18next"
import { CloneWikiDialog } from "@/components/project/clone-wiki-dialog"

import { Brand } from "@/design-system/components"
import { LibraryAccountMenu } from "@/components/setup/library-account-menu"

interface WelcomeScreenProps {
  onCreateProject: () => void
  onOpenProject: () => void
  onSelectProject: (project: WikiProject) => void
  onAccount: () => void
  onQuickStart: () => void
}

export function WelcomeScreen({
  onCreateProject,
  onOpenProject,
  onSelectProject,
  onAccount,
  onQuickStart,
}: WelcomeScreenProps) {
  const { t } = useTranslation()
  const [recentProjects, setRecentProjects] = useState<WikiProject[]>([])
  const [cloning, setCloning] = useState(false)

  useEffect(() => {
    getRecentProjects()
      .then(setRecentProjects)
      .catch(() => {})
  }, [])

  async function handleRemoveRecent(e: React.MouseEvent, path: string) {
    e.stopPropagation()
    await removeFromRecentProjects(path)
    const updated = await getRecentProjects()
    setRecentProjects(updated)
  }

  return (
    <div className="desktop-welcome">
      <header className="desktop-welcome-header">
        <Brand />
        <LibraryAccountMenu />
      </header>
      <main className="desktop-welcome-main">
        <section className="desktop-welcome-intro">
          <h1>
            Your knowledge.
            <br />
            Ready for what's next.
          </h1>
          <p>
            Open a project, connect your coding agent, and keep what you learn
            in a wiki you own.
          </p>
          <div className="desktop-welcome-actions">
            <Button onClick={onCreateProject}>
              <Plus className="h-4 w-4" />
              {t("welcome.newProject")}
            </Button>
            <Button variant="outline" onClick={onOpenProject}>
              <FolderOpen className="h-4 w-4" />
              {t("welcome.openProject")}
            </Button>
          </div>
          <button
            className="desktop-welcome-link"
            onClick={() => setCloning(true)}
          >
            <Cloud className="h-4 w-4" />
            Clone from Library
            <ArrowRight className="h-4 w-4" />
          </button>
          <div className="desktop-welcome-help">
            <button onClick={onQuickStart}>
              <CircleHelp className="h-4 w-4" />
              Quick start guide
            </button>
            <button onClick={onAccount}>Your Library account</button>
          </div>
        </section>
        <section className="desktop-recent" aria-labelledby="recent-heading">
          <h2 id="recent-heading">
            <Clock className="h-4 w-4" />
            {t("welcome.recentProjects")}
          </h2>
          {recentProjects.length ? (
            <ul>
              {recentProjects.map((proj) => (
                <li key={proj.path}>
                  <button
                    className="desktop-recent-open"
                    onClick={() => onSelectProject(proj)}
                  >
                    <FolderOpen className="h-5 w-5" />
                    <span>
                      <strong>{proj.name}</strong>
                      <small>{proj.path}</small>
                    </span>
                    <ArrowRight className="h-4 w-4" />
                  </button>
                  <button
                    className="desktop-recent-remove"
                    aria-label={`Remove ${proj.name} from recent projects`}
                    onClick={(event) =>
                      void handleRemoveRecent(event, proj.path)
                    }
                  >
                    <X className="h-4 w-4" />
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <div className="desktop-recent-empty">
              <FolderOpen className="h-8 w-8" />
              <h3>A place for your projects</h3>
              <p>Projects you open will appear here, ready to pick up again.</p>
            </div>
          )}
        </section>
      </main>
      <CloneWikiDialog
        open={cloning}
        onOpenChange={setCloning}
        onCloned={onSelectProject}
      />
    </div>
  )
}
