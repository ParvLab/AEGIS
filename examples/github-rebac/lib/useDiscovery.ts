import { useState, useEffect, useCallback } from "react";

export interface DiscoveryData {
  subjects: string[];
  objects: string[];
  relations: string[];
  subjectTypes: string[];
  objectTypes: string[];
  permissions: string[];
  loading: boolean;
}

export function useDiscovery() {
  const [data, setData] = useState<DiscoveryData>({
    subjects: [],
    objects: [],
    relations: [],
    subjectTypes: [],
    objectTypes: [],
    permissions: [],
    loading: true,
  });

  const refresh = useCallback(async () => {
    try {
      const res = await fetch("/api/discovery");
      if (res.ok) {
        const json = await res.json();
        setData({
          subjects: json.subjects ?? [],
          objects: json.objects ?? [],
          relations: json.relations ?? [],
          subjectTypes: json.subjectTypes ?? [],
          objectTypes: json.objectTypes ?? [],
          permissions: json.permissions ?? [],
          loading: false,
        });
      } else {
        setData(prev => ({ ...prev, loading: false }));
      }
    } catch {
      setData(prev => ({ ...prev, loading: false }));
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  return { ...data, refresh };
}
