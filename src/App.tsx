import { useEffect, useState } from 'react';
import { Sidebar, type View } from '@/components/Sidebar';
import { Header } from '@/components/Header';
import { DashboardView } from '@/features/dashboard/DashboardView';
import { ProfilesView } from '@/features/profiles/ProfilesView';
import { SettingsView } from '@/features/settings/SettingsView';
import { LogsView } from '@/features/logs/LogsView';
import { Toaster } from '@/components/ui/Toaster';
import { useProfileStore } from '@/store/useProfileStore';
import { useSettingsStore } from '@/store/useSettingsStore';

export default function App() {
  const [view, setView] = useState<View>('instances');
  const initProfiles = useProfileStore((s) => s.init);
  const loadSettings = useSettingsStore((s) => s.load);

  useEffect(() => {
    void loadSettings();
    const unsubscribe = initProfiles();
    return unsubscribe;
  }, [initProfiles, loadSettings]);

  return (
    <div className="relative flex h-screen w-screen overflow-hidden">
      <Sidebar current={view} onNavigate={setView} />
      <div className="flex flex-1 flex-col overflow-hidden pl-[68px]">
        <Header />
        <main className="flex flex-1 flex-col overflow-hidden animate-fade-in">
          {view === 'dashboard' && <DashboardView />}
          {view === 'instances' && <ProfilesView />}
          {view === 'settings' && <SettingsView />}
          {view === 'logs' && <LogsView />}
        </main>
      </div>
      <Toaster />
    </div>
  );
}
