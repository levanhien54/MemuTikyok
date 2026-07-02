import { useEffect, useState } from 'react';
import { Sidebar, type View } from '@/components/Sidebar';
import { Header } from '@/components/Header';
import { DashboardView } from '@/features/dashboard/DashboardView';
import { InstancesView } from '@/features/instances/InstancesView';
import { SettingsView } from '@/features/settings/SettingsView';
import { LogsView } from '@/features/logs/LogsView';
import { Toaster } from '@/components/ui/Toaster';
import { useInstanceStore } from '@/store/useInstanceStore';
import { useSettingsStore } from '@/store/useSettingsStore';

export default function App() {
  const [view, setView] = useState<View>('instances');
  const initInstances = useInstanceStore((s) => s.init);
  const loadSettings = useSettingsStore((s) => s.load);

  useEffect(() => {
    void loadSettings();
    const unsubscribe = initInstances();
    return unsubscribe;
  }, [initInstances, loadSettings]);

  return (
    <div className="relative flex h-screen w-screen overflow-hidden">
      <Sidebar current={view} onNavigate={setView} />
      <div className="flex flex-1 flex-col overflow-hidden pl-[68px]">
        <Header />
        <main className="flex flex-1 flex-col overflow-hidden animate-fade-in">
          {view === 'dashboard' && <DashboardView />}
          {view === 'instances' && <InstancesView />}
          {view === 'settings' && <SettingsView />}
          {view === 'logs' && <LogsView />}
        </main>
      </div>
      <Toaster />
    </div>
  );
}
