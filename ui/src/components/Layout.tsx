import { Outlet, NavLink, useLocation } from "react-router-dom";
import {
  Box,
  Flex,
  Text,
  Container,
  DropdownMenu,
  IconButton,
  Badge,
  Separator,
} from "@radix-ui/themes";
import {
  MagnifyingGlassIcon,
  CounterClockwiseClockIcon,
  SunIcon,
  MoonIcon,
  PersonIcon,
  GearIcon,
  ExitIcon,
  CodeIcon,
} from "@radix-ui/react-icons";
import { AnimatePresence, motion } from "framer-motion";
import { useAuth } from "../hooks/useAuth";
import { useDebug } from "../hooks/useDebug";
import { DebugPane } from "./DebugPane";

interface LayoutProps {
  theme: "dark" | "light";
  toggleTheme: () => void;
}

const navLinkStyle = (isActive: boolean) => ({
  textDecoration: "none",
  color: "inherit",
  opacity: isActive ? 1 : 0.7,
  fontWeight: isActive ? 600 : 400,
});

export function Layout({ theme, toggleTheme }: LayoutProps) {
  const { user, logout } = useAuth();
  const { debug, setDebug } = useDebug();
  const location = useLocation();

  return (
    <Flex direction="column" style={{ minHeight: "100vh" }}>
      <Box
        asChild
        px="4"
        py="3"
        style={{
          borderBottom: "1px solid var(--gray-a5)",
          backdropFilter: "blur(12px)",
          position: "sticky",
          top: 0,
          zIndex: 100,
          background: "var(--color-panel-translucent)",
        }}
      >
        <header>
          <Container size="4">
            <Flex align="center" justify="between">
              <Flex align="center" gap="5">
                <NavLink to="/" style={{ textDecoration: "none", color: "inherit" }}>
                  <Flex align="center" gap="2">
                    <img src="/icons/logo.svg" alt="StreamX" width={28} height={28} />
                    <Text size="4" weight="bold">
                      StreamX
                    </Text>
                  </Flex>
                </NavLink>

                <Separator orientation="vertical" size="1" />

                <Flex gap="4" align="center">
                  <NavLink
                    to="/"
                    end
                    style={({ isActive }) => navLinkStyle(isActive)}
                  >
                    <Flex align="center" gap="1">
                      <MagnifyingGlassIcon />
                      <Text size="2">Search</Text>
                    </Flex>
                  </NavLink>
                  <NavLink
                    to="/history"
                    style={({ isActive }) => navLinkStyle(isActive)}
                  >
                    <Flex align="center" gap="1">
                      <CounterClockwiseClockIcon />
                      <Text size="2">History</Text>
                    </Flex>
                  </NavLink>
                </Flex>
              </Flex>

              <Flex align="center" gap="3">
                <IconButton
                  variant="ghost"
                  size="2"
                  onClick={toggleTheme}
                  aria-label="Toggle theme"
                >
                  {theme === "dark" ? <SunIcon /> : <MoonIcon />}
                </IconButton>

                <DropdownMenu.Root>
                  <DropdownMenu.Trigger>
                    <IconButton variant="ghost" size="2" aria-label="User menu">
                      <PersonIcon />
                    </IconButton>
                  </DropdownMenu.Trigger>
                  <DropdownMenu.Content align="end">
                    <DropdownMenu.Label>
                      <Flex align="center" gap="2">
                        <Text>{user?.username ?? "User"}</Text>
                        {user?.is_admin && (
                          <Badge size="1" color="amber">
                            Admin
                          </Badge>
                        )}
                      </Flex>
                    </DropdownMenu.Label>
                    <DropdownMenu.Separator />
                    <DropdownMenu.Item asChild>
                      <NavLink to="/settings" style={{ textDecoration: "none", color: "inherit" }}>
                        <GearIcon />
                        Settings
                      </NavLink>
                    </DropdownMenu.Item>
                    <DropdownMenu.Item onClick={() => setDebug(!debug)}>
                      <CodeIcon />
                      Debug Mode
                      {debug && (
                        <Badge size="1" color="green" ml="2">
                          ON
                        </Badge>
                      )}
                    </DropdownMenu.Item>
                    <DropdownMenu.Separator />
                    <DropdownMenu.Item color="red" onClick={logout}>
                      <ExitIcon />
                      Logout
                    </DropdownMenu.Item>
                  </DropdownMenu.Content>
                </DropdownMenu.Root>
              </Flex>
            </Flex>
          </Container>
        </header>
      </Box>

      <Box flexGrow="1" py="4">
        <Container size="4" px="4">
          {location.pathname.startsWith("/player") ? (
            <Outlet />
          ) : (
            <AnimatePresence mode="wait">
              <motion.div
                key={location.pathname}
                initial={{ opacity: 0, y: 8 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -8 }}
                transition={{ duration: 0.15 }}
              >
                <Outlet />
              </motion.div>
            </AnimatePresence>
          )}
        </Container>
      </Box>

      {debug && <DebugPane onClose={() => setDebug(false)} />}
    </Flex>
  );
}
